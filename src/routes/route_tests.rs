use axum::{
    Router,
    body::to_bytes,
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;

use crate::{app::build_app, config::AppConfig};

fn test_app() -> Router {
    build_app(AppConfig::default()).unwrap()
}

#[tokio::test]
async fn healthz_returns_ok() {
    let response = test_app()
        .oneshot(
            Request::get("/healthz")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn api_check_rejects_malformed_url() {
    let response = test_app()
        .oneshot(
            Request::post("/api/check")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"url":"not a url"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["code"], "invalid_url");
}

#[tokio::test]
async fn api_pan115_share_list_rejects_malformed_url() {
    let response = test_app()
        .oneshot(
            Request::post("/api/115/share/list")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"url":"not a url","list_type":"files"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["code"], "invalid_url");
}

#[tokio::test]
async fn api_pan123_share_list_rejects_malformed_url() {
    let response = test_app()
        .oneshot(
            Request::post("/api/123/share/list")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"url":"not a url","list_type":"files"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["code"], "invalid_url");
}

#[tokio::test]
async fn demo_page_renders() {
    let response = test_app()
        .oneshot(
            Request::get("/demo")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("115 / 123 Share Demo"));
    assert!(html.contains("/api/115/share/list"));
    assert!(html.contains("/api/123/share/list"));
    assert!(html.contains("share-provider"));
}
