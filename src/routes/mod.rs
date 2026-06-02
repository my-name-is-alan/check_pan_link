pub mod api;
pub mod demo;
pub mod health;
pub mod telegram;

use axum::{
    Router,
    http::{HeaderValue, Method, header::CONTENT_TYPE},
    routing::{get, post},
};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::app::AppState;

pub const HEALTH_PATH: &str = "/healthz";
pub const API_CHECK_PATH: &str = "/api/check";
pub const API_PAN115_SHARE_LIST_PATH: &str = "/api/115/share/list";
pub const API_PAN123_SHARE_LIST_PATH: &str = "/api/123/share/list";
pub const DEMO_PATH: &str = "/demo";
pub const TELEGRAM_WEBHOOK_PATH: &str = "/telegram/webhook";

#[cfg(test)]
mod route_tests;

pub fn router(state: AppState) -> Router {
    let cors = cors_layer(state.config.cors_allowed_origin.as_deref());

    Router::new()
        .route(HEALTH_PATH, get(health::healthz))
        .route(API_CHECK_PATH, post(api::check))
        .route(API_PAN115_SHARE_LIST_PATH, post(api::list_pan115_share))
        .route(API_PAN123_SHARE_LIST_PATH, post(api::list_pan123_share))
        .route(DEMO_PATH, get(demo::page))
        .route(TELEGRAM_WEBHOOK_PATH, post(telegram::webhook))
        .with_state(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
}

fn cors_layer(allowed_origin: Option<&str>) -> CorsLayer {
    let layer = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([CONTENT_TYPE]);

    match allowed_origin {
        Some("*") => layer.allow_origin(Any),
        Some(origin) => match HeaderValue::from_str(origin) {
            Ok(origin) => layer.allow_origin(origin),
            Err(_) => layer,
        },
        None => layer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_namespaces_preserve_future_ui_space() {
        assert!(API_CHECK_PATH.starts_with("/api/"));
        assert!(API_PAN115_SHARE_LIST_PATH.starts_with("/api/"));
        assert!(API_PAN123_SHARE_LIST_PATH.starts_with("/api/"));
        assert!(TELEGRAM_WEBHOOK_PATH.starts_with("/telegram/"));
        assert!(!HEALTH_PATH.starts_with("/api/"));
        assert!(!HEALTH_PATH.starts_with("/telegram/"));
        assert!(!DEMO_PATH.starts_with("/api/"));
        assert!(!DEMO_PATH.starts_with("/telegram/"));
    }
}
