use axum::{Json, extract::State};

use crate::{
    app::AppState,
    checker::{CheckRequest, CheckResult, Pan115ShareListRequest, Pan115ShareListResponse},
    error::ApiError,
};

pub async fn check(
    State(state): State<AppState>,
    Json(payload): Json<CheckRequest>,
) -> Result<Json<CheckResult>, ApiError> {
    let result = state.checker.check(payload).await?;
    Ok(Json(result))
}

pub async fn list_pan115_share(
    State(state): State<AppState>,
    Json(payload): Json<Pan115ShareListRequest>,
) -> Result<Json<Pan115ShareListResponse>, ApiError> {
    let result = state.checker.list_pan115_share(payload).await?;
    Ok(Json(result))
}
