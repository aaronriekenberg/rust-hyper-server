mod commands;
mod connection;
mod request_info;
mod route;
mod utils;

use std::sync::Arc;

use async_trait::async_trait;

use hyper::{http::Response, Body};

use crate::{connection::ConnectionTracker, request::HttpRequest};

#[async_trait]
pub trait RequestHandler: Send + Sync {
    async fn handle(&self, request: &HttpRequest) -> Response<Body>;
}

pub fn create_handlers(
    connection_tracker: &Arc<ConnectionTracker>,
) -> anyhow::Result<Box<dyn RequestHandler>> {
    let mut routes = Vec::new();

    routes.append(&mut connection::create_routes(connection_tracker));

    routes.append(&mut commands::create_routes()?);

    routes.append(&mut request_info::create_routes());

    Ok(Box::new(route::Router::new(routes)?))
}
