use hyper::{
    http::{Request, Response},
    server::conn::http1::Builder as HyperHTTP1Builder,
    server::conn::http2::Builder as HyperHTTP2Builder,
    service::service_fn,
};

use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::Duration,
};

use tracing::{debug, info, instrument, warn, Instrument};

use pin_project::pin_project;

use std::{convert::Infallible, pin::Pin, sync::Arc};

use crate::{
    config::ServerProtocol,
    connection::{ConnectionGuard, ConnectionID},
    handlers::RequestHandler,
    request::{HttpRequest, RequestID, RequestIDFactory},
    response::ResponseBody,
};

pub struct ConnectionHandler {
    request_handler: Box<dyn RequestHandler>,
    request_id_factory: RequestIDFactory,
    connection_timeout_durations: Vec<Duration>,
}

impl ConnectionHandler {
    pub fn new(
        request_handler: Box<dyn RequestHandler>,
        request_id_factory: RequestIDFactory,
    ) -> Arc<Self> {
        let server_configuration = crate::config::instance().server_configuration();

        let connection_timeout_durations = vec![
            server_configuration.connection_max_lifetime(),
            server_configuration.connection_graceful_shutdown_timeout(),
        ];

        debug!(
            "connection_timeout_durations = {:?}",
            connection_timeout_durations
        );

        Arc::new(Self {
            request_handler,
            request_id_factory,
            connection_timeout_durations,
        })
    }

    #[instrument(skip_all, fields(req_id = request_id.as_usize()))]
    async fn handle_request(
        self: Arc<Self>,
        connection_id: ConnectionID,
        request_id: RequestID,
        hyper_request: Request<hyper::body::Incoming>,
    ) -> Result<Response<ResponseBody>, Infallible> {
        debug!("begin handle_request");

        let http_request = HttpRequest::new(connection_id, request_id, hyper_request);

        let result = self.request_handler.handle(&http_request).await;

        debug!("end handle_request");
        Ok(result)
    }

    #[instrument(skip_all, fields(
        conn_id = connection.id().as_usize(),
        sock = ?connection.server_socket_type(),
        proto = ?connection.server_protocol(),
    ))]
    pub async fn handle_connection(
        self: Arc<Self>,
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
        connection: ConnectionGuard,
    ) {
        info!("begin handle_connection");

        let service = service_fn(|hyper_request| {
            connection.increment_num_requests();

            let request_id = self.request_id_factory.new_request_id();

            Arc::clone(&self)
                .handle_request(connection.id(), request_id, hyper_request)
                .in_current_span()
        });

        let mut wrapped_conn = match connection.server_protocol() {
            ServerProtocol::Http1 => {
                let conn = HyperHTTP1Builder::new().serve_connection(stream, service);
                WrappedHyperConnection::H1(conn)
            }
            ServerProtocol::Http2 => {
                let conn = HyperHTTP2Builder::new(TokioExecutor).serve_connection(stream, service);
                WrappedHyperConnection::H2(conn)
            }
        };

        let mut wrapped_conn = Pin::new(&mut wrapped_conn);

        for (iter, sleep_duration) in self.connection_timeout_durations.iter().enumerate() {
            debug!("iter = {} sleep_duration = {:?}", iter, sleep_duration);
            tokio::select! {
                res = wrapped_conn.as_mut() => {
                    match res {
                        Ok(()) => debug!("after polling conn, no error"),
                        Err(e) =>  warn!("error serving connection: {:?}", e),
                    };
                    break;
                }
                _ = tokio::time::sleep(*sleep_duration) => {
                    info!("iter = {} got timeout_interval, calling conn.graceful_shutdown", iter);
                    wrapped_conn.as_mut().graceful_shutdown();
                }
            }
        }

        info!("end handle_connection");
    }
}

#[derive(Clone)]
struct TokioExecutor;

impl<F> hyper::rt::Executor<F> for TokioExecutor
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    fn execute(&self, fut: F) {
        tokio::task::spawn(fut);
    }
}

#[pin_project(project = WrappedHyperConnectionProj)]
enum WrappedHyperConnection<I, S, E>
where
    I: AsyncRead + AsyncWrite + Unpin + 'static,
    S: hyper::service::HttpService<hyper::body::Incoming, ResBody = ResponseBody>,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    E: hyper::rt::bounds::Http2ConnExec<S::Future, ResponseBody>,
{
    H1(#[pin] hyper::server::conn::http1::Connection<I, S>),
    H2(#[pin] hyper::server::conn::http2::Connection<I, S, E>),
}

impl<I, S, E> std::future::Future for WrappedHyperConnection<I, S, E>
where
    I: AsyncRead + AsyncWrite + Unpin + 'static,
    S: hyper::service::HttpService<hyper::body::Incoming, ResBody = ResponseBody>,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    E: hyper::rt::bounds::Http2ConnExec<S::Future, ResponseBody>,
{
    type Output = hyper::Result<()>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match self.project() {
            WrappedHyperConnectionProj::H1(h1_conn) => h1_conn.poll(cx),
            WrappedHyperConnectionProj::H2(h2_conn) => h2_conn.poll(cx),
        }
    }
}

impl<I, S, E> WrappedHyperConnection<I, S, E>
where
    I: AsyncRead + AsyncWrite + Unpin + 'static,
    S: hyper::service::HttpService<hyper::body::Incoming, ResBody = ResponseBody>,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    E: hyper::rt::bounds::Http2ConnExec<S::Future, ResponseBody>,
{
    pub fn graceful_shutdown(self: Pin<&mut Self>) {
        match self.project() {
            WrappedHyperConnectionProj::H1(h1_conn) => h1_conn.graceful_shutdown(),
            WrappedHyperConnectionProj::H2(h2_conn) => h2_conn.graceful_shutdown(),
        }
    }
}
