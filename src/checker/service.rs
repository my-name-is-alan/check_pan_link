use std::time::Duration;

use reqwest::redirect::Policy;
use url::Url;

use crate::{
    checker::model::{
        CheckRequest, CheckResult, Pan115ShareListRequest, Pan115ShareListResponse, Provider,
    },
    error::{ApiError, CheckError},
    providers::{CheckContext, ProviderRegistry, common::host_is_or_subdomain, pan115_share},
};

#[cfg(test)]
use crate::error::ShareListError;

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
        let url = parse_http_url(&request.url)?;
        let context = CheckContext {
            original_url: request.url,
            url,
            client: self.client.clone(),
        };

        self.providers.check(context).await
    }

    pub async fn list_pan115_share(
        &self,
        request: Pan115ShareListRequest,
    ) -> Result<Pan115ShareListResponse, ApiError> {
        let context = self.build_pan115_context(request.url)?;

        pan115_share::list_share(context, request.list_type)
            .await
            .map_err(Into::into)
    }

    pub fn detect_provider(&self, url: &Url) -> Provider {
        let normalized = normalize_pan115_share_url(url);
        self.providers.detect(&normalized)
    }

    fn build_pan115_context(&self, original_url: String) -> Result<CheckContext, ApiError> {
        let url = parse_http_url(&original_url).map_err(ApiError::from)?;

        if self.detect_provider(&url) != Provider::Pan115 {
            return Err(ApiError::bad_request(
                "invalid_pan115_share_url",
                "expected a 115 share URL like https://115cdn.com/s/<share_code>?password=<code> or https://anxia.com/s/<share_code>?password=<code>",
            ));
        }

        Ok(CheckContext {
            original_url,
            url,
            client: self.client.clone(),
        })
    }

    #[cfg(test)]
    pub(crate) async fn list_pan115_share_with_endpoint(
        &self,
        request: Pan115ShareListRequest,
        endpoint: &str,
    ) -> Result<Pan115ShareListResponse, ShareListError> {
        let url =
            parse_http_url(&request.url).map_err(|_| ShareListError::InvalidPan115ShareUrl)?;

        if self.detect_provider(&url) != Provider::Pan115 {
            return Err(ShareListError::InvalidPan115ShareUrl);
        }

        let context = CheckContext {
            original_url: request.url,
            url,
            client: self.client.clone(),
        };

        pan115_share::list_share_with_endpoint(context, request.list_type, endpoint).await
    }
}

fn parse_http_url(original_url: &str) -> Result<Url, CheckError> {
    let raw_url = original_url.trim();
    let url = Url::parse(raw_url).map_err(|_| CheckError::InvalidUrl(original_url.to_string()))?;

    match url.scheme() {
        "http" | "https" => Ok(normalize_pan115_share_url(&url)),
        scheme => Err(CheckError::UnsupportedScheme(scheme.to_string())),
    }
}

fn normalize_pan115_share_url(url: &Url) -> Url {
    if !host_is_or_subdomain(url, "anxia.com") {
        return url.clone();
    }

    let Some(share_code) = extract_pan115_share_code(url) else {
        return url.clone();
    };

    let mut normalized = Url::parse(&format!("https://115cdn.com/s/{share_code}"))
        .expect("canonical 115 share url should be valid");

    if let Some(receive_code) = extract_pan115_receive_code(url) {
        normalized
            .query_pairs_mut()
            .append_pair("password", &receive_code);
    }

    normalized
}

fn extract_pan115_share_code(url: &Url) -> Option<String> {
    let mut segments = url.path_segments()?;
    match (segments.next(), segments.next()) {
        (Some("s"), Some(share_code)) if !share_code.is_empty() => Some(share_code.to_string()),
        _ => None,
    }
}

fn extract_pan115_receive_code(url: &Url) -> Option<String> {
    url.query_pairs()
        .find(|(key, value)| {
            matches!(key.as_ref(), "password" | "receive_code") && !value.is_empty()
        })
        .map(|(_, value)| value.into_owned())
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
            checker.detect_provider(
                &Url::parse("https://anxia.com/s/swh5dd83nwq?password=6969#").unwrap()
            ),
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

    #[test]
    fn normalizes_anxia_share_to_canonical_115cdn_url() {
        let url = Url::parse("https://anxia.com/s/swh5dd83nwq?password=6969#").unwrap();
        let normalized = normalize_pan115_share_url(&url);

        assert_eq!(
            normalized.as_str(),
            "https://115cdn.com/s/swh5dd83nwq?password=6969"
        );
    }
}
