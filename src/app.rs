use std::sync::Arc;

use axum::Router;
use teloxide::Bot;

use crate::{checker::LinkCheckerService, config::AppConfig, error::CheckError, routes};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub checker: Arc<LinkCheckerService>,
    pub telegram_bot: Option<Bot>,
}

impl AppState {
    pub fn try_new(config: AppConfig) -> Result<Self, CheckError> {
        let telegram_bot = config.telegram_bot_token.as_ref().map(Bot::new);
        let checker = Arc::new(LinkCheckerService::new(config.check_timeout)?);

        Ok(Self {
            config: Arc::new(config),
            checker,
            telegram_bot,
        })
    }
}

pub fn build_app(config: AppConfig) -> Result<Router, CheckError> {
    let state = AppState::try_new(config)?;
    Ok(routes::router(state))
}
