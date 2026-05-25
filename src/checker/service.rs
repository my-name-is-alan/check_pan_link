use std::time::Duration;

use reqwest::redirect::Policy;
use url::Url;

use crate::{
    checker::model::{CheckRequest, CheckResult, Provider},
    error::CheckError,
    providers::{CheckContext, ProviderRegistry},
};

#[derive(Clone)]
pub struct LinkCheckerService {
    client: reqwest::Client,
    providers: ProviderRegistry,
}

impl LinkCheckerService {
    pub fn new(timeout: Duration) -> Result<Self, CheckError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(Policy::limited(5))
            .user_agent("check-pan-link/0.1")
            .build()?;

        Ok(Self {
            client,
            providers: ProviderRegistry::default(),
        })
    }

    pub async fn check(&self, request: CheckRequest) -> Result<CheckResult, CheckError> {
        let raw_url = request.url.trim();
        let url = Url::parse(raw_url).map_err(|_| CheckError::InvalidUrl(request.url.clone()))?;

        match url.scheme() {
            "http" | "https" => {}
            scheme => return Err(CheckError::UnsupportedScheme(scheme.to_string())),
        }

        let context = CheckContext {
            original_url: request.url,
            url,
            client: self.client.clone(),
        };

        self.providers.check(context).await
    }

    pub fn detect_provider(&self, url: &Url) -> Provider {
        self.providers.detect(url)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use url::Url;

    use super::*;

    #[tokio::test]
    async fn rejects_invalid_url() {
        let checker = LinkCheckerService::new(Duration::from_secs(1)).unwrap();
        let error = checker
            .check(CheckRequest {
                url: "not a url".to_string(),
            })
            .await
            .unwrap_err();

        assert!(matches!(error, CheckError::InvalidUrl(_)));
    }

    #[tokio::test]
    async fn rejects_unsupported_scheme() {
        let checker = LinkCheckerService::new(Duration::from_secs(1)).unwrap();
        let error = checker
            .check(CheckRequest {
                url: "ftp://example.com/file.txt".to_string(),
            })
            .await
            .unwrap_err();

        assert!(matches!(error, CheckError::UnsupportedScheme(_)));
    }

    #[test]
    fn detects_provider_without_network_request() {
        let checker = LinkCheckerService::new(Duration::from_secs(1)).unwrap();

        assert_eq!(
            checker.detect_provider(&Url::parse("https://115.com/s/example").unwrap()),
            Provider::Pan115
        );
        assert_eq!(
            checker.detect_provider(&Url::parse("https://cloud.189.cn/t/example").unwrap()),
            Provider::Pan189
        );
        assert_eq!(
            checker.detect_provider(&Url::parse("https://www.123pan.com/s/example").unwrap()),
            Provider::Pan123
        );
        assert_eq!(
            checker.detect_provider(&Url::parse("https://example.com/share").unwrap()),
            Provider::Generic
        );
    }
}
