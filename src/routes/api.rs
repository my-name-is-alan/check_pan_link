use axum::{Json, extract::State};

use crate::{
    app::AppState,
    checker::{CheckRequest, CheckResult},
    error::ApiError,
};

pub async fn check(
    State(state): State<AppState>,
    Json(payload): Json<CheckRequest>,
) -> Result<Json<CheckResult>, ApiError> {
    let result = state.checker.check(payload).await?;
    Ok(Json(result))
}
