use std::time::Duration;

use reqwest::redirect::Policy;
use url::Url;

use crate::{
    checker::model::{
        CheckRequest, CheckResult, GuangyaShareListRequest, GuangyaShareListResponse,
        Pan115ShareListRequest, Pan115ShareListResponse, Pan123ShareListRequest,
        Pan123ShareListResponse, Pan189ShareListRequest, Pan189ShareListResponse, Provider,
    },
    error::{ApiError, CheckError},
    providers::{
        CheckContext, ProviderRegistry, common::host_is_or_subdomain, guangya_share, pan115_share,
        pan123_share, pan189_share,
    },
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

    #[cfg(test)]
    pub(crate) fn new_without_proxy(timeout: Duration) -> Result<Self, CheckError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(Policy::limited(5))
            .user_agent("check-pan-link/0.1")
            .no_proxy()
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

    pub async fn list_pan123_share(
        &self,
        request: Pan123ShareListRequest,
    ) -> Result<Pan123ShareListResponse, ApiError> {
        let context = self.build_pan123_context(request.url)?;

        pan123_share::list_share(context, request.list_type)
            .await
            .map_err(Into::into)
    }

    pub async fn list_pan189_share(
        &self,
        request: Pan189ShareListRequest,
    ) -> Result<Pan189ShareListResponse, ApiError> {
        let context = self.build_pan189_context(request.url)?;

        pan189_share::list_share(context, request.list_type)
            .await
            .map_err(Into::into)
    }

    pub async fn list_guangya_share(
        &self,
        request: GuangyaShareListRequest,
    ) -> Result<GuangyaShareListResponse, ApiError> {
        let context = self.build_guangya_context(request.url)?;

        guangya_share::list_share(context, request.list_type)
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

    fn build_pan123_context(&self, original_url: String) -> Result<CheckContext, ApiError> {
        let url = parse_http_url(&original_url).map_err(ApiError::from)?;

        if self.detect_provider(&url) != Provider::Pan123 {
            return Err(ApiError::bad_request(
                "invalid_pan123_share_url",
                "expected a 123 share URL like https://www.123865.com/s/<share_key>?pwd=<code> or https://www.123pan.com/s/<share_key>?pwd=<code>",
            ));
        }

        Ok(CheckContext {
            original_url,
            url,
            client: self.client.clone(),
        })
    }

    fn build_pan189_context(&self, original_url: String) -> Result<CheckContext, ApiError> {
        let url = parse_http_url(&original_url).map_err(ApiError::from)?;

        if self.detect_provider(&url) != Provider::Pan189 {
            return Err(ApiError::bad_request(
                "invalid_pan189_share_url",
                "expected an 189 share URL like https://cloud.189.cn/t/<share_code>?accessCode=<code> or https://cloud.189.cn/web/share?code=<share_code>",
            ));
        }

        Ok(CheckContext {
            original_url,
            url,
            client: self.client.clone(),
        })
    }

    fn build_guangya_context(&self, original_url: String) -> Result<CheckContext, ApiError> {
        let url = parse_http_url(&original_url).map_err(ApiError::from)?;

        if self.detect_provider(&url) != Provider::GuangyaPan {
            return Err(ApiError::bad_request(
                "invalid_guangya_share_url",
                "expected a Guangya share URL like https://www.guangyapan.com/s/<share_id>?code=<code>",
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

    #[cfg(test)]
    pub(crate) async fn list_pan123_share_with_endpoints(
        &self,
        request: Pan123ShareListRequest,
        share_info_endpoint: &str,
        share_get_endpoint: &str,
    ) -> Result<Pan123ShareListResponse, ShareListError> {
        let url =
            parse_http_url(&request.url).map_err(|_| ShareListError::InvalidPan123ShareUrl)?;

        if self.detect_provider(&url) != Provider::Pan123 {
            return Err(ShareListError::InvalidPan123ShareUrl);
        }

        let context = CheckContext {
            original_url: request.url,
            url,
            client: self.client.clone(),
        };

        pan123_share::list_share_with_endpoints(
            context,
            request.list_type,
            share_info_endpoint,
            share_get_endpoint,
        )
        .await
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
            checker.detect_provider(
                &Url::parse("https://www.123865.com/s/IpPUVv-gGKj?pwd=Ocat").unwrap()
            ),
            Provider::Pan123
        );
        assert_eq!(
            checker.detect_provider(
                &Url::parse("https://www.guangyapan.com/s/1910961250145955889_adz3Lo8EdLN_2BBy",)
                    .unwrap()
            ),
            Provider::GuangyaPan
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
