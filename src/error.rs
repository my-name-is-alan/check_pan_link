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
    #[error("invalid 123 share URL")]
    InvalidPan123ShareUrl,
    #[error("invalid 189 share URL")]
    InvalidPan189ShareUrl,
    #[error("invalid Guangya share URL")]
    InvalidGuangyaShareUrl,
    #[error("share requires receive code")]
    MissingReceiveCode,
    #[error("189 share requires access code")]
    MissingAccessCode,
    #[error("share receive code is invalid")]
    InvalidReceiveCode,
    #[error("share code is invalid")]
    InvalidShareCode,
    #[error("share has expired")]
    ShareExpired,
    #[error("failed to request share list: {0}")]
    RequestFailed(String),
    #[error("failed to parse share list response: {0}")]
    ParseFailed(String),
    #[error("share list API returned an error: {0}")]
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
            ShareListError::InvalidPan123ShareUrl => Self::bad_request(
                "invalid_pan123_share_url",
                "expected a 123 share URL like https://www.123865.com/s/<share_key>?pwd=<code> or https://www.123pan.com/s/<share_key>?pwd=<code>",
            ),
            ShareListError::InvalidPan189ShareUrl => Self::bad_request(
                "invalid_pan189_share_url",
                "expected an 189 share URL like https://cloud.189.cn/t/<share_code>?accessCode=<code> or https://cloud.189.cn/web/share?code=<share_code>",
            ),
            ShareListError::InvalidGuangyaShareUrl => Self::bad_request(
                "invalid_guangya_share_url",
                "expected a Guangya share URL like https://www.guangyapan.com/s/<share_id>?code=<code>",
            ),
            ShareListError::MissingReceiveCode => {
                Self::bad_request("missing_receive_code", "share requires a receive code")
            }
            ShareListError::MissingAccessCode => Self::bad_request(
                "missing_access_code",
                "189 share requires accessCode; URL code= is the share code, not the access code",
            ),
            ShareListError::InvalidReceiveCode => {
                Self::bad_request("invalid_receive_code", "share receive code is invalid")
            }
            ShareListError::InvalidShareCode => {
                Self::bad_request("invalid_share_code", "share code is invalid or inactive")
            }
            ShareListError::ShareExpired => Self::bad_request("share_expired", "share has expired"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_missing_access_code_to_pan189_specific_api_error() {
        let error = ApiError::from(ShareListError::MissingAccessCode);

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.code, "missing_access_code");
        assert!(error.message.contains("accessCode"));
        assert!(error.message.contains("code="));
    }
}
