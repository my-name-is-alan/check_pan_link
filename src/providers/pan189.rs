use async_trait::async_trait;
use url::Url;

use crate::{
    checker::{CheckResult, Provider},
    error::CheckError,
    providers::common::{CheckContext, ProviderChecker, basic_http_check, host_is_or_subdomain},
};

pub struct Pan189Checker;

#[async_trait]
impl ProviderChecker for Pan189Checker {
    fn provider(&self) -> Provider {
        Provider::Pan189
    }

    fn matches(&self, url: &Url) -> bool {
        ["cloud.189.cn", "h5.cloud.189.cn"]
            .iter()
            .any(|domain| host_is_or_subdomain(url, domain))
    }

    async fn check(&self, context: CheckContext) -> Result<CheckResult, CheckError> {
        basic_http_check(context, self.provider()).await
    }
}
