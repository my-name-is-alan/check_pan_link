use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use teloxide::types::Update;

use crate::{app::AppState, error::ApiError, telegram};

const TELEGRAM_SECRET_HEADER: &str = "x-telegram-bot-api-secret-token";

pub async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(update): Json<Update>,
) -> Result<StatusCode, ApiError> {
    validate_secret(&state, &headers)?;

    let Some(bot) = state.telegram_bot.clone() else {
        return Err(ApiError::service_unavailable(
            "telegram_disabled",
            "TELOXIDE_TOKEN is required for Telegram replies",
        ));
    };

    telegram::handle_update(update, bot, state.checker.clone())
        .await
        .map_err(|error| ApiError::bad_gateway("telegram_reply_failed", error.to_string()))?;

    Ok(StatusCode::OK)
}

fn validate_secret(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(expected) = state.config.telegram_webhook_secret.as_deref() else {
        return Ok(());
    };

    let actual = headers
        .get(TELEGRAM_SECRET_HEADER)
        .and_then(|value| value.to_str().ok());

    if actual == Some(expected) {
        Ok(())
    } else {
        Err(ApiError::unauthorized(
            "telegram_secret_mismatch",
            "invalid Telegram webhook secret",
        ))
    }
}
