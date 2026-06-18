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
async fn api_pan189_share_list_rejects_malformed_url() {
    let response = test_app()
        .oneshot(
            Request::post("/api/189/share/list")
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
async fn api_guangya_share_list_rejects_malformed_url() {
    let response = test_app()
        .oneshot(
            Request::post("/api/guangya/share/list")
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
    assert!(html.contains("115 / 123 / 189 / 光鸭 Share Demo"));
    assert!(html.contains("/api/115/share/list"));
    assert!(html.contains("/api/123/share/list"));
    assert!(html.contains("/api/189/share/list"));
    assert!(html.contains("/api/guangya/share/list"));
    assert!(html.contains("share-provider"));
    assert!(html.contains("光鸭云盘"));
    assert!(html.contains("guangya_pan"));
    assert!(
        html.contains(
            "https://www.guangyapan.com/s/1910961611262906426_adz3Lo8EdLN_2BBy?code=bmiu"
        )
    );
    assert!(html.contains("URL 里的 code 是分享码，不是访问码"));
}
