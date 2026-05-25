use std::{env, num::ParseIntError, time::Duration};

use thiserror::Error;

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8080;
const DEFAULT_CHECK_TIMEOUT_SECS: u64 = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub telegram_bot_token: Option<String>,
    pub telegram_webhook_secret: Option<String>,
    pub check_timeout: Duration,
    pub cors_allowed_origin: Option<String>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let host = optional_env("APP_HOST").unwrap_or_else(|| DEFAULT_HOST.to_string());
        let port = parse_u16_env("APP_PORT", DEFAULT_PORT)?;
        let check_timeout_secs = parse_u64_env("CHECK_TIMEOUT_SECS", DEFAULT_CHECK_TIMEOUT_SECS)?;

        Ok(Self {
            host,
            port,
            telegram_bot_token: optional_env("TELOXIDE_TOKEN"),
            telegram_webhook_secret: optional_env("TELEGRAM_WEBHOOK_SECRET"),
            check_timeout: Duration::from_secs(check_timeout_secs),
            cors_allowed_origin: optional_env("CORS_ALLOWED_ORIGIN"),
        })
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            telegram_bot_token: None,
            telegram_webhook_secret: None,
            check_timeout: Duration::from_secs(DEFAULT_CHECK_TIMEOUT_SECS),
            cors_allowed_origin: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid {name}: {source}")]
    InvalidInteger {
        name: &'static str,
        #[source]
        source: ParseIntError,
    },
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_u16_env(name: &'static str, default: u16) -> Result<u16, ConfigError> {
    match optional_env(name) {
        Some(value) => value
            .parse()
            .map_err(|source| ConfigError::InvalidInteger { name, source }),
        None => Ok(default),
    }
}

fn parse_u64_env(name: &'static str, default: u64) -> Result<u64, ConfigError> {
    match optional_env(name) {
        Some(value) => value
            .parse()
            .map_err(|source| ConfigError::InvalidInteger { name, source }),
        None => Ok(default),
    }
}
