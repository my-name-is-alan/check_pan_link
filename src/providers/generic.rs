use async_trait::async_trait;
use url::Url;

use crate::{
    checker::{CheckResult, Provider},
    error::CheckError,
    providers::common::{CheckContext, ProviderChecker, basic_http_check},
};

pub struct GenericChecker;

#[async_trait]
impl ProviderChecker for GenericChecker {
    fn provider(&self) -> Provider {
        Provider::Generic
    }

    fn matches(&self, _url: &Url) -> bool {
        true
    }

    async fn check(&self, context: CheckContext) -> Result<CheckResult, CheckError> {
        basic_http_check(context, self.provider()).await
    }
}
