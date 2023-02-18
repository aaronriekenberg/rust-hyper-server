use std::{convert::From, path::PathBuf};

use async_trait::async_trait;

use chrono::prelude::SecondsFormat;

use hyper::{Body, Method, Response};

use serde::Serialize;

use crate::{
    config::ServerProtocol,
    connection::{ConnectionInfo, ConnectionTracker},
    handlers::{route::RouteInfo, utils::build_json_response, HttpRequest, RequestHandler},
};

#[derive(Debug, Serialize)]
struct ConnectionInfoDTO {
    connection_id: u64,
    creation_time: String,
    server_protocol: ServerProtocol,
}

impl From<&ConnectionInfo> for ConnectionInfoDTO {
    fn from(connection_info: &ConnectionInfo) -> Self {
        Self {
            connection_id: connection_info.connection_id().0,
            creation_time: connection_info
                .creation_time()
                .to_rfc3339_opts(SecondsFormat::Nanos, true),
            server_protocol: *connection_info.server_protocol(),
        }
    }
}

#[derive(Debug, Serialize)]
struct ConnectionInfoResponse {
    connections: Vec<ConnectionInfoDTO>,
}

struct ConnectionInfoHandler {
    connection_tracker: &'static ConnectionTracker,
}

impl ConnectionInfoHandler {
    async fn new() -> Self {
        Self {
            connection_tracker: crate::connection::get_connection_tracker().await,
        }
    }
}

#[async_trait]
impl RequestHandler for ConnectionInfoHandler {
    async fn handle(&self, _request: &HttpRequest) -> Response<Body> {
        let response = ConnectionInfoResponse {
            connections: self
                .connection_tracker
                .get_all_connections()
                .iter()
                .map(|c| c.into())
                .collect(),
        };

        build_json_response(response)
    }
}

pub async fn create_routes() -> Vec<RouteInfo> {
    vec![RouteInfo {
        method: &Method::GET,
        path_suffix: PathBuf::from("connection_info"),
        handler: Box::new(ConnectionInfoHandler::new().await),
    }]
}