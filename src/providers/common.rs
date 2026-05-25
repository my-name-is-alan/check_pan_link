use std::collections::BTreeMap;

use async_trait::async_trait;
use reqwest::header::RANGE;
use serde_json::json;
use url::Url;

use crate::{
    checker::{CheckResult, CheckStatus, Provider},
    error::CheckError,
};

#[derive(Clone)]
pub struct CheckContext {
    pub original_url: String,
    pub url: Url,
    pub client: reqwest::Client,
}

#[async_trait]
pub trait ProviderChecker: Send + Sync {
    fn provider(&self) -> Provider;
    fn matches(&self, url: &Url) -> bool;
    async fn check(&self, context: CheckContext) -> Result<CheckResult, CheckError>;
}

pub fn host_is_or_subdomain(url: &Url, domain: &str) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };

    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();

    host == domain || host.ends_with(&format!(".{domain}"))
}

pub async fn basic_http_check(
    context: CheckContext,
    provider: Provider,
) -> Result<CheckResult, CheckError> {
    let original_url = context.original_url;
    let normalized_url = context.url.to_string();
    let response = context
        .client
        .get(context.url)
        .header(RANGE, "bytes=0-0")
        .send()
        .await;

    match response {
        Ok(response) => {
            let status_code = response.status();
            let mut metadata = BTreeMap::new();
            metadata.insert("http_status".to_string(), json!(status_code.as_u16()));
            metadata.insert("final_url".to_string(), json!(response.url().to_string()));

            let (status, reason) = if status_code.is_success() || status_code.is_redirection() {
                (CheckStatus::Valid, "http_success")
            } else if matches!(status_code.as_u16(), 404 | 410 | 451) {
                (CheckStatus::Invalid, "http_not_found")
            } else if status_code.is_client_error() {
                (CheckStatus::Unknown, "http_client_error")
            } else if status_code.is_server_error() {
                (CheckStatus::Unknown, "http_server_error")
            } else {
                (CheckStatus::Unknown, "http_unclassified")
            };

            Ok(CheckResult::new(
                original_url,
                normalized_url,
                status,
                provider,
                reason,
                metadata,
            ))
        }
        Err(error) => {
            let reason = if error.is_timeout() {
                "request_timeout"
            } else if error.is_redirect() {
                "redirect_error"
            } else if error.is_connect() {
                "connection_failed"
            } else {
                "request_failed"
            };

            let mut metadata = BTreeMap::new();
            metadata.insert("error".to_string(), json!(error.to_string()));

            Ok(CheckResult::new(
                original_url,
                normalized_url,
                CheckStatus::Unknown,
                provider,
                reason,
                metadata,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_host_boundaries() {
        assert!(host_is_or_subdomain(
            &Url::parse("https://cloud.189.cn/t/example").unwrap(),
            "cloud.189.cn"
        ));
        assert!(host_is_or_subdomain(
            &Url::parse("https://share.115.com/s/example").unwrap(),
            "115.com"
        ));
        assert!(!host_is_or_subdomain(
            &Url::parse("https://evil115.com/s/example").unwrap(),
            "115.com"
        ));
    }
}
