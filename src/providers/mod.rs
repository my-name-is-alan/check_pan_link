pub mod common;
pub mod generic;
pub mod pan115;
pub mod pan123;
pub mod pan189;

use std::sync::Arc;

use url::Url;

use crate::{
    checker::{CheckResult, Provider},
    error::CheckError,
    providers::{
        generic::GenericChecker, pan115::Pan115Checker, pan123::Pan123Checker,
        pan189::Pan189Checker,
    },
};

pub use common::{CheckContext, ProviderChecker};

#[derive(Clone)]
pub struct ProviderRegistry {
    checkers: Vec<Arc<dyn ProviderChecker>>,
}

impl ProviderRegistry {
    pub async fn check(&self, context: CheckContext) -> Result<CheckResult, CheckError> {
        self.find(&context.url).check(context).await
    }

    pub fn detect(&self, url: &Url) -> Provider {
        self.find(url).provider()
    }

    fn find(&self, url: &Url) -> Arc<dyn ProviderChecker> {
        self.checkers
            .iter()
            .find(|checker| checker.matches(url))
            .cloned()
            .unwrap_or_else(|| Arc::new(GenericChecker))
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self {
            checkers: vec![
                Arc::new(Pan115Checker),
                Arc::new(Pan189Checker),
                Arc::new(Pan123Checker),
                Arc::new(GenericChecker),
            ],
        }
    }
}
