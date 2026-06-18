use axum::{Json, extract::State};

use crate::{
    app::AppState,
    checker::{
        CheckRequest, CheckResult, GuangyaShareListRequest, GuangyaShareListResponse,
        Pan115ShareListRequest, Pan115ShareListResponse, Pan123ShareListRequest,
        Pan123ShareListResponse, Pan189ShareListRequest, Pan189ShareListResponse,
    },
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

pub async fn list_pan123_share(
    State(state): State<AppState>,
    Json(payload): Json<Pan123ShareListRequest>,
) -> Result<Json<Pan123ShareListResponse>, ApiError> {
    let result = state.checker.list_pan123_share(payload).await?;
    Ok(Json(result))
}

pub async fn list_pan189_share(
    State(state): State<AppState>,
    Json(payload): Json<Pan189ShareListRequest>,
) -> Result<Json<Pan189ShareListResponse>, ApiError> {
    let result = state.checker.list_pan189_share(payload).await?;
    Ok(Json(result))
}

pub async fn list_guangya_share(
    State(state): State<AppState>,
    Json(payload): Json<GuangyaShareListRequest>,
) -> Result<Json<GuangyaShareListResponse>, ApiError> {
    let result = state.checker.list_guangya_share(payload).await?;
    Ok(Json(result))
}
