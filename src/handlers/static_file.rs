use async_trait::async_trait;

use http_body_util::BodyExt;

use hyper::http::{Response, StatusCode};

use hyper_staticfile::{vfs::TokioFileOpener, ResolveResult, Resolver};

use tracing::{debug, warn};

use std::{path::Path, time::SystemTime};

use tokio::time::Duration;

use crate::{
    handlers::{
        response_utils::{build_premanent_redirect_response, build_status_code_response},
        HttpRequest, RequestHandler, ResponseBody,
    },
    response::CacheControl,
};

const DEFAULT_CACHE_DURATION_SECONDS: u32 = 60 * 60;

const VNSTAT_PNG_CACHE_DURATION: Duration = Duration::from_secs(15 * 60);

struct StaticFileHandler {
    resolver: Resolver<TokioFileOpener>,
    client_error_page_path: &'static str,
}

impl StaticFileHandler {
    fn new() -> Self {
        let static_file_configuration = crate::config::instance().static_file_configuration();
        let root = Path::new(static_file_configuration.path());

        let mut resolver = Resolver::new(root);
        resolver.allowed_encodings.gzip = static_file_configuration.precompressed_gz();
        resolver.allowed_encodings.br = static_file_configuration.precompressed_br();

        debug!(
            "resolver.allowed_encodings = {:?}",
            resolver.allowed_encodings
        );

        Self {
            resolver,
            client_error_page_path: static_file_configuration.client_error_page_path(),
        }
    }

    fn build_client_error_page_response(&self) -> Response<ResponseBody> {
        build_premanent_redirect_response(self.client_error_page_path, CacheControl::NoCache)
    }

    fn handle_resolve_errors(
        &self,
        resolve_result: &ResolveResult,
    ) -> Option<Response<ResponseBody>> {
        match resolve_result {
            ResolveResult::MethodNotMatched => Some(build_status_code_response(
                StatusCode::BAD_REQUEST,
                CacheControl::NoCache,
            )),
            ResolveResult::NotFound | ResolveResult::PermissionDenied => {
                Some(self.build_client_error_page_response())
            }
            _ => None,
        }
    }

    fn block_dot_paths(&self, resolve_result: &ResolveResult) -> Option<Response<ResponseBody>> {
        let str_path_option = match resolve_result {
            ResolveResult::Found(resolved_file) => resolved_file.path.to_str(),
            ResolveResult::IsDirectory { redirect_to } => Some(redirect_to.as_str()),
            _ => None,
        };

        if let Some(str_path) = str_path_option {
            debug!("str_path = {}", str_path);
            if str_path.starts_with('.') || str_path.contains("/.") {
                warn!("blocking request for dot file path = {:?}", str_path);
                return Some(self.build_client_error_page_response());
            }
        };

        None
    }

    fn build_cache_headers(&self, resolve_result: &ResolveResult) -> Option<u32> {
        match resolve_result {
            ResolveResult::Found(resolved_file) => {
                debug!("resolved_file.path = {:?}", resolved_file.path,);

                let str_path = resolved_file.path.to_str().unwrap_or_default();

                if !(str_path.contains("vnstat/") && str_path.ends_with(".png")) {
                    Some(DEFAULT_CACHE_DURATION_SECONDS)
                } else {
                    debug!("request for vnstat png file path");

                    match resolved_file.modified {
                        None => Some(0),
                        Some(modified) => {
                            let now = SystemTime::now();

                            let file_expiration = modified + VNSTAT_PNG_CACHE_DURATION;

                            let cache_duration =
                                file_expiration.duration_since(now).unwrap_or_default();

                            debug!(
                                "file_expiration = {:?} cache_duration = {:?}",
                                file_expiration, cache_duration
                            );

                            Some(cache_duration.as_secs().try_into().unwrap_or_default())
                        }
                    }
                }
            }
            _ => None,
        }
    }
}

#[async_trait]
impl RequestHandler for StaticFileHandler {
    async fn handle(&self, request: &HttpRequest) -> Response<ResponseBody> {
        debug!("handle_static_file request = {:?}", request);

        let resolve_result = match self.resolver.resolve_request(request.hyper_request()).await {
            Ok(resolve_result) => resolve_result,
            Err(e) => {
                warn!("resolve_request error e = {}", e);
                return build_status_code_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    CacheControl::NoCache,
                );
            }
        };

        debug!("resolve_result = {:?}", resolve_result);

        if let Some(response) = self.handle_resolve_errors(&resolve_result) {
            return response;
        }

        if let Some(response) = self.block_dot_paths(&resolve_result) {
            return response;
        }

        let cache_headers = self.build_cache_headers(&resolve_result);

        debug!("cache_headers = {:?}", cache_headers);

        let response = match hyper_staticfile::ResponseBuilder::new()
            .request(request.hyper_request())
            .cache_headers(cache_headers)
            .build(resolve_result)
        {
            Ok(response) => response,
            Err(e) => {
                warn!("ResponseBuilder.build error e = {}", e);
                return build_status_code_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    CacheControl::NoCache,
                );
            }
        };

        let (parts, body) = response.into_parts();

        let boxed_body = body.map_err(|e| e.into()).boxed();

        Response::from_parts(parts, boxed_body)
    }
}

pub async fn create_default_route() -> Box<dyn RequestHandler> {
    Box::new(StaticFileHandler::new())
}
