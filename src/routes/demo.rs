use axum::response::Html;

const DEMO_PAGE: &str = include_str!("demo.html");

pub async fn page() -> Html<&'static str> {
    Html(DEMO_PAGE)
}
