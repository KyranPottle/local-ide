use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{self, HeaderValue};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::routes::api_error;

pub struct AccessPolicy {
    pub allowed_peers: Vec<IpAddr>,
    pub public_origin: String,
}

pub const WEBSOCKET_PATH: &str = "/api/ws";

pub async fn enforce(
    State(policy): State<Arc<AccessPolicy>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    if !policy.allowed_peers.contains(&peer.ip().to_canonical()) {
        tracing::warn!(%peer, "rejected a peer outside allowed_peers");
        return api_error(StatusCode::FORBIDDEN, "peer not allowed").into_response();
    }
    let needs_origin = request.uri().path() == WEBSOCKET_PATH
        || !matches!(
            *request.method(),
            Method::GET | Method::HEAD | Method::OPTIONS
        );
    let origin_matches = request
        .headers()
        .get(header::ORIGIN)
        .is_some_and(|origin| origin.as_bytes() == policy.public_origin.as_bytes());
    if needs_origin && !origin_matches {
        return api_error(StatusCode::FORBIDDEN, "origin not allowed").into_response();
    }
    next.run(request).await
}

const SECURITY_HEADERS: [(header::HeaderName, &str); 3] = [
    (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
    (header::REFERRER_POLICY, "no-referrer"),
    (
        header::CONTENT_SECURITY_POLICY,
        "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'",
    ),
];

pub async fn add_security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    for (name, value) in SECURITY_HEADERS {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    response
}
