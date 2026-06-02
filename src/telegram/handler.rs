use std::sync::Arc;

use teloxide::{
    Bot, RequestError,
    prelude::{Request, Requester},
    types::{Message, Update, UpdateKind},
};
use thiserror::Error;

use crate::{
    checker::{CheckRequest, CheckResult, CheckStatus, LinkCheckerService},
    error::CheckError,
};

#[derive(Debug, Error)]
pub enum TelegramError {
    #[error("telegram request failed: {0}")]
    Request(#[from] RequestError),
}

pub async fn handle_update(
    update: Update,
    bot: Bot,
    checker: Arc<LinkCheckerService>,
) -> Result<(), TelegramError> {
    let UpdateKind::Message(message) = update.kind else {
        return Ok(());
    };

    let Some(text) = message.text() else {
        return Ok(());
    };

    let Some(command) = parse_check_command(text) else {
        return Ok(());
    };

    let reply = match command {
        CheckCommand::MissingUrl => "Usage: /check <url>".to_string(),
        CheckCommand::Url(url) => match checker.check(CheckRequest { url }).await {
            Ok(result) => format_check_reply(&result),
            Err(error) => format_check_error(&error),
        },
    };

    send_reply(bot, &message, reply).await
}

enum CheckCommand {
    MissingUrl,
    Url(String),
}

fn parse_check_command(text: &str) -> Option<CheckCommand> {
    let trimmed = text.trim();
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let command = parts.next()?;

    if command != "/check" && !command.starts_with("/check@") {
        return None;
    }

    let rest = parts.next().unwrap_or_default().trim();
    if rest.is_empty() {
        Some(CheckCommand::MissingUrl)
    } else {
        Some(CheckCommand::Url(rest.to_string()))
    }
}

fn format_check_reply(result: &CheckResult) -> String {
    format!(
        "Status: {}\nProvider: {}\nReason: {}\nURL: {}",
        status_text(&result.status),
        result.provider,
        result.reason,
        result.normalized_url
    )
}

fn format_check_error(error: &CheckError) -> String {
    format!("Check failed: {error}")
}

fn status_text(status: &CheckStatus) -> &'static str {
    match status {
        CheckStatus::Valid => "valid",
        CheckStatus::Invalid => "invalid",
        CheckStatus::Processing => "processing",
        CheckStatus::Unknown => "unknown",
    }
}

async fn send_reply(bot: Bot, message: &Message, reply: String) -> Result<(), TelegramError> {
    bot.send_message(message.chat.id, reply).send().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_check_command() {
        assert!(matches!(
            parse_check_command("/check"),
            Some(CheckCommand::MissingUrl)
        ));
        assert!(matches!(
            parse_check_command("/check https://example.com"),
            Some(CheckCommand::Url(url)) if url == "https://example.com"
        ));
        assert!(matches!(
            parse_check_command("/check@my_bot https://example.com"),
            Some(CheckCommand::Url(url)) if url == "https://example.com"
        ));
        assert!(parse_check_command("/start").is_none());
    }
}
