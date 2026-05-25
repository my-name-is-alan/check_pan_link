use check_pan_link::{app, config::AppConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let config = AppConfig::from_env()?;
    let bind_addr = config.bind_addr();
    let app = app::build_app(config)?;
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    tracing::info!(%bind_addr, "listening");
    axum::serve(listener, app).await?;

    Ok(())
}
