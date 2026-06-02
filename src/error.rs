use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CheckError {
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("unsupported URL scheme: {0}")]
    UnsupportedScheme(String),
    #[error("failed to build HTTP client: {0}")]
    HttpClient(#[from] reqwest::Error),
}

#[derive(Debug, Error)]
pub enum ShareListError {
    #[error("invalid 115 share URL")]
    InvalidPan115ShareUrl,
    #[error("share requires receive code")]
    MissingReceiveCode,
    #[error("share receive code is invalid")]
    InvalidReceiveCode,
    #[error("share code is invalid")]
    InvalidShareCode,
    #[error("failed to request 115 share list: {0}")]
    RequestFailed(String),
    #[error("failed to parse 115 share list response: {0}")]
    ParseFailed(String),
    #[error("115 share list API returned an error: {0}")]
    Api(String),
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub fn unauthorized(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code,
            message: message.into(),
        }
    }

    pub fn service_unavailable(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code,
            message: message.into(),
        }
    }

    pub fn bad_gateway(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            code,
            message: message.into(),
        }
    }

    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code,
            message: message.into(),
        }
    }
}

impl From<CheckError> for ApiError {
    fn from(value: CheckError) -> Self {
        match value {
            CheckError::InvalidUrl(message) => Self {
                status: StatusCode::BAD_REQUEST,
                code: "invalid_url",
                message,
            },
            CheckError::UnsupportedScheme(message) => Self {
                status: StatusCode::BAD_REQUEST,
                code: "unsupported_scheme",
                message,
            },
            CheckError::HttpClient(error) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "http_client_error",
                message: error.to_string(),
            },
        }
    }
}

impl From<ShareListError> for ApiError {
    fn from(value: ShareListError) -> Self {
        match value {
            ShareListError::InvalidPan115ShareUrl => Self::bad_request(
                "invalid_pan115_share_url",
                "expected a 115 share URL like https://115cdn.com/s/<share_code>?password=<code> or https://anxia.com/s/<share_code>?password=<code>",
            ),
            ShareListError::MissingReceiveCode => {
                Self::bad_request("missing_receive_code", "115 share requires a receive code")
            }
            ShareListError::InvalidReceiveCode => {
                Self::bad_request("invalid_receive_code", "115 receive code is invalid")
            }
            ShareListError::InvalidShareCode => {
                Self::bad_request("invalid_share_code", "115 share code is invalid")
            }
            ShareListError::RequestFailed(message) => {
                Self::bad_gateway("share_list_request_failed", message)
            }
            ShareListError::ParseFailed(message) => {
                Self::bad_gateway("share_list_parse_failed", message)
            }
            ShareListError::Api(message) => Self::bad_gateway("share_list_api_error", message),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorResponse {
            error: ErrorBody {
                code: self.code,
                message: self.message,
            },
        };

        (self.status, Json(body)).into_response()
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}
