use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use url::Url;

use crate::{
    checker::{CheckResult, CheckStatus, Provider},
    error::CheckError,
    providers::common::{CheckContext, ProviderChecker, basic_http_check, host_is_or_subdomain},
};

const SHARE_INFO_ENDPOINT: &str =
    "https://cloud.189.cn/api/open/share/getShareInfoByCodeV2.action";
const CHECK_ACCESS_CODE_ENDPOINT: &str =
    "https://cloud.189.cn/api/open/share/checkAccessCode.action";

const RES_CODE_SHARE_NOT_FOUND: &str = "ShareNotFound";
const RES_CODE_SHARE_AUDIT_NOT_PASS: &str = "ShareAuditNotPass";

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
        let Some(share_code) = extract_share_code(&context.url) else {
            return basic_http_check(context, self.provider()).await;
        };

        let access_code = extract_access_code(&context.url);
        let normalized_url = normalize_share_url(&share_code, access_code.as_deref());

        Ok(
            check_share(context, share_code, access_code, normalized_url, self.provider())
                .await,
        )
    }
}

async fn check_share(
    context: CheckContext,
    share_code: String,
    access_code: Option<String>,
    normalized_url: String,
    provider: Provider,
) -> CheckResult {
    let original_url = context.original_url;
    let mut metadata = base_metadata(&share_code, access_code.as_deref());
    metadata.insert("api_endpoint".to_string(), json!(SHARE_INFO_ENDPOINT));

    let request_url = build_share_info_url(&share_code);
    let request = with_share_headers(context.client.get(request_url), &share_code);

    match request.send().await {
        Ok(response) => {
            metadata.insert(
                "api_http_status".to_string(),
                json!(response.status().as_u16()),
            );

            match response.json::<ShareInfoResponse>().await {
                Ok(body) => build_share_info_result(
                    context.client,
                    original_url,
                    normalized_url,
                    provider,
                    metadata,
                    share_code,
                    access_code,
                    body,
                )
                .await,
                Err(error) => {
                    metadata.insert("error".to_string(), json!(error.to_string()));
                    CheckResult::new(
                        original_url,
                        normalized_url,
                        CheckStatus::Unknown,
                        provider,
                        "share_info_parse_failed",
                        metadata,
                    )
                }
            }
        }
        Err(error) => {
            let reason = if error.is_timeout() {
                "share_info_timeout"
            } else if error.is_connect() {
                "share_info_connection_failed"
            } else {
                "share_info_request_failed"
            };

            metadata.insert("error".to_string(), json!(error.to_string()));

            CheckResult::new(
                original_url,
                normalized_url,
                CheckStatus::Unknown,
                provider,
                reason,
                metadata,
            )
        }
    }
}

async fn build_share_info_result(
    client: reqwest::Client,
    original_url: String,
    normalized_url: String,
    provider: Provider,
    mut metadata: BTreeMap<String, serde_json::Value>,
    share_code: String,
    access_code: Option<String>,
    body: ShareInfoResponse,
) -> CheckResult {
    metadata.insert("api_res_code".to_string(), json!(body.res_code.as_value()));
    metadata.insert(
        "api_res_code_label".to_string(),
        json!(res_code_label(&body.res_code)),
    );

    if !body.res_message.is_empty() {
        metadata.insert("api_res_message".to_string(), json!(body.res_message));
    }

    if let Some(need_access_code) = body.need_access_code {
        metadata.insert("need_access_code".to_string(), json!(need_access_code));
    }

    if let Some(review_status) = body.review_status {
        metadata.insert("review_status".to_string(), json!(review_status));
        metadata.insert(
            "review_status_label".to_string(),
            json!(review_status_label(review_status)),
        );
    }

    if !body.file_name.is_empty() {
        metadata.insert("file_name".to_string(), json!(body.file_name));
    }

    if !body.file_id.is_empty() {
        metadata.insert("file_id".to_string(), json!(body.file_id));
    }

    if let Some(is_folder) = body.is_folder {
        metadata.insert("is_folder".to_string(), json!(is_folder));
    }

    match classify_share_info(&body, access_code.is_some()) {
        ShareInfoDecision::Return(status, reason) => CheckResult::new(
            original_url,
            normalized_url,
            status,
            provider,
            reason,
            metadata,
        ),
        ShareInfoDecision::VerifyAccessCode => {
            verify_access_code(
                client,
                original_url,
                normalized_url,
                provider,
                metadata,
                share_code,
                access_code.expect("access code presence already checked"),
            )
            .await
        }
    }
}

async fn verify_access_code(
    client: reqwest::Client,
    original_url: String,
    normalized_url: String,
    provider: Provider,
    mut metadata: BTreeMap<String, serde_json::Value>,
    share_code: String,
    access_code: String,
) -> CheckResult {
    let request_url = Url::parse_with_params(
        CHECK_ACCESS_CODE_ENDPOINT,
        &[
            ("returnType", "json"),
            ("shareCode", share_code.as_str()),
            ("accessCode", access_code.as_str()),
        ],
    )
    .expect("pan189 access code endpoint should always be valid");

    metadata.insert(
        "access_code_verify_endpoint".to_string(),
        json!(CHECK_ACCESS_CODE_ENDPOINT),
    );

    let request = with_share_headers(client.get(request_url), &share_code);

    match request.send().await {
        Ok(response) => {
            metadata.insert(
                "access_code_verify_http_status".to_string(),
                json!(response.status().as_u16()),
            );

            match response.json::<CheckAccessCodeResponse>().await {
                Ok(body) => {
                    metadata.insert(
                        "access_code_verify_res_code".to_string(),
                        json!(body.res_code.as_value()),
                    );

                    if !body.res_message.is_empty() {
                        metadata.insert(
                            "access_code_verify_res_message".to_string(),
                            json!(body.res_message),
                        );
                    }

                    if let Some(share_id) = body.share_id {
                        metadata.insert("share_id".to_string(), json!(share_id));
                    }

                    if let Some(success) = body.success {
                        metadata.insert("access_code_verified".to_string(), json!(success));
                    }

                    let (status, reason) = classify_access_code(&body);

                    CheckResult::new(
                        original_url,
                        normalized_url,
                        status,
                        provider,
                        reason,
                        metadata,
                    )
                }
                Err(error) => {
                    metadata.insert("error".to_string(), json!(error.to_string()));
                    CheckResult::new(
                        original_url,
                        normalized_url,
                        CheckStatus::Unknown,
                        provider,
                        "access_code_verify_parse_failed",
                        metadata,
                    )
                }
            }
        }
        Err(error) => {
            let reason = if error.is_timeout() {
                "access_code_verify_timeout"
            } else if error.is_connect() {
                "access_code_verify_connection_failed"
            } else {
                "access_code_verify_request_failed"
            };

            metadata.insert("error".to_string(), json!(error.to_string()));

            CheckResult::new(
                original_url,
                normalized_url,
                CheckStatus::Unknown,
                provider,
                reason,
                metadata,
            )
        }
    }
}

fn classify_share_info(body: &ShareInfoResponse, access_code_provided: bool) -> ShareInfoDecision {
    if body.res_code.is_text(RES_CODE_SHARE_NOT_FOUND) {
        return ShareInfoDecision::Return(CheckStatus::Invalid, "share_not_found");
    }

    if body.res_code.is_text(RES_CODE_SHARE_AUDIT_NOT_PASS) {
        return ShareInfoDecision::Return(CheckStatus::Invalid, "share_audit_blocked");
    }

    if !body.res_code.is_success() {
        return ShareInfoDecision::Return(CheckStatus::Unknown, "share_info_api_error");
    }

    if body
        .review_status
        .is_some_and(|review_status| review_status != 1)
    {
        return ShareInfoDecision::Return(CheckStatus::Invalid, "share_review_blocked");
    }

    if body.need_access_code.is_some_and(|need_access_code| need_access_code == 1) {
        if access_code_provided {
            ShareInfoDecision::VerifyAccessCode
        } else {
            ShareInfoDecision::Return(CheckStatus::Invalid, "missing_access_code")
        }
    } else {
        ShareInfoDecision::Return(CheckStatus::Valid, "share_available")
    }
}

fn classify_access_code(body: &CheckAccessCodeResponse) -> (CheckStatus, &'static str) {
    if body.res_code.is_text(RES_CODE_SHARE_NOT_FOUND) {
        return (CheckStatus::Invalid, "share_not_found");
    }

    if body.res_code.is_text(RES_CODE_SHARE_AUDIT_NOT_PASS) {
        return (CheckStatus::Invalid, "share_audit_blocked");
    }

    if !body.res_code.is_success() {
        return (CheckStatus::Unknown, "access_code_verify_api_error");
    }

    if body.share_id.is_some() {
        return (CheckStatus::Valid, "share_available");
    }

    if body.success == Some(false) {
        return (CheckStatus::Invalid, "invalid_access_code");
    }

    (CheckStatus::Unknown, "access_code_verify_inconclusive")
}

fn build_share_info_url(share_code: &str) -> Url {
    Url::parse_with_params(
        SHARE_INFO_ENDPOINT,
        &[("returnType", "json"), ("shareCode", share_code)],
    )
    .expect("pan189 share info endpoint should always be valid")
}

fn with_share_headers(
    request: reqwest::RequestBuilder,
    share_code: &str,
) -> reqwest::RequestBuilder {
    request
        .header("Accept", "application/json;charset=UTF-8")
        .header("Sign-Type", "1")
        .header(
            "Referer",
            format!("https://cloud.189.cn/web/share?code={share_code}"),
        )
}

fn normalize_share_url(share_code: &str, access_code: Option<&str>) -> String {
    let mut normalized =
        Url::parse(&format!("https://cloud.189.cn/t/{share_code}"))
            .expect("canonical 189 share url should be valid");

    if let Some(access_code) = access_code {
        normalized
            .query_pairs_mut()
            .append_pair("accessCode", access_code);
    }

    normalized.to_string()
}

fn base_metadata(
    share_code: &str,
    access_code: Option<&str>,
) -> BTreeMap<String, serde_json::Value> {
    let mut metadata = BTreeMap::new();
    metadata.insert("share_code".to_string(), json!(share_code));
    metadata.insert(
        "access_code_provided".to_string(),
        json!(access_code.is_some()),
    );

    if let Some(access_code) = access_code {
        metadata.insert("access_code".to_string(), json!(access_code));
    }

    metadata
}

fn extract_share_code(url: &Url) -> Option<String> {
    let share_code = parse_share_code(&extract_raw_share_token(url)?);

    if share_code.is_empty() {
        None
    } else {
        Some(share_code)
    }
}

fn extract_raw_share_token(url: &Url) -> Option<String> {
    if let Some(raw) = extract_raw_share_code_from_query(url) {
        return Some(raw);
    }

    let mut segments = url.path_segments()?;
    match (segments.next(), segments.next()) {
        (Some("t"), Some(raw)) if !raw.is_empty() => Some(raw.to_string()),
        _ => None,
    }
}

fn extract_raw_share_code_from_query(url: &Url) -> Option<String> {
    let path = url.path().trim_end_matches('/');
    if path != "/web/share" {
        return None;
    }

    url.query_pairs()
        .find(|(key, value)| key == "code" && !value.is_empty())
        .map(|(_, value)| value.into_owned())
}

fn extract_access_code(url: &Url) -> Option<String> {
    extract_access_code_from_query(url).or_else(|| {
        extract_raw_share_token(url).and_then(|raw| extract_embedded_access_code(&raw))
    })
}

fn extract_access_code_from_query(url: &Url) -> Option<String> {
    url.query_pairs()
        .find(|(key, value)| {
            matches!(
                key.as_ref(),
                "accessCode" | "access_code" | "password" | "pwd" | "receive_code"
            ) && !value.is_empty()
        })
        .map(|(_, value)| value.into_owned())
}

fn parse_share_code(raw: &str) -> String {
    raw.trim()
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

fn extract_embedded_access_code(raw: &str) -> Option<String> {
    for keyword in ["访问码", "提取码", "密码"] {
        if let Some(access_code) = extract_access_code_after_keyword(raw, keyword) {
            return Some(access_code);
        }
    }

    extract_access_code_after_keyword_ci(raw, "code")
}

fn extract_access_code_after_keyword(raw: &str, keyword: &str) -> Option<String> {
    let index = raw.find(keyword)?;
    let rest = &raw[index + keyword.len()..];

    parse_access_code_token(rest)
}

fn extract_access_code_after_keyword_ci(raw: &str, keyword: &str) -> Option<String> {
    let lower_raw = raw.to_ascii_lowercase();
    let lower_keyword = keyword.to_ascii_lowercase();
    let index = lower_raw.find(&lower_keyword)?;
    let rest = &raw[index + keyword.len()..];

    parse_access_code_token(rest)
}

fn parse_access_code_token(text: &str) -> Option<String> {
    let trimmed = text.trim_start_matches(access_code_prefix_char);

    let access_code: String = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric())
        .collect();

    if access_code.len() == 4 {
        Some(access_code)
    } else {
        None
    }
}

fn access_code_prefix_char(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '：' | ':' | '(' | '（' | ')' | '）')
}

fn res_code_label(res_code: &ApiResCode) -> &'static str {
    match res_code {
        ApiResCode::Number(0) => "ok",
        ApiResCode::Text(code) if code == RES_CODE_SHARE_NOT_FOUND => "share_not_found",
        ApiResCode::Text(code) if code == RES_CODE_SHARE_AUDIT_NOT_PASS => "share_audit_blocked",
        ApiResCode::Text(_) => "api_error",
        ApiResCode::Number(_) => "api_error",
        ApiResCode::Missing => "missing",
    }
}

fn review_status_label(review_status: i64) -> &'static str {
    match review_status {
        1 => "normal",
        _ => "blocked",
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ShareInfoDecision {
    Return(CheckStatus, &'static str),
    VerifyAccessCode,
}

#[derive(Debug, Deserialize)]
struct ShareInfoResponse {
    #[serde(rename = "res_code", default)]
    res_code: ApiResCode,
    #[serde(rename = "res_message", default)]
    res_message: String,
    #[serde(rename = "needAccessCode", default)]
    need_access_code: Option<i64>,
    #[serde(rename = "reviewStatus", default)]
    review_status: Option<i64>,
    #[serde(rename = "fileName", default)]
    file_name: String,
    #[serde(rename = "fileId", default)]
    file_id: String,
    #[serde(rename = "isFolder", default)]
    is_folder: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CheckAccessCodeResponse {
    #[serde(rename = "res_code", default)]
    res_code: ApiResCode,
    #[serde(rename = "res_message", default)]
    res_message: String,
    #[serde(rename = "shareId", default)]
    share_id: Option<i64>,
    #[serde(default)]
    success: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum ApiResCode {
    Number(i64),
    Text(String),
    #[default]
    Missing,
}

impl ApiResCode {
    fn is_success(&self) -> bool {
        matches!(self, Self::Number(0))
    }

    fn is_text(&self, expected: &str) -> bool {
        matches!(self, Self::Text(code) if code == expected)
    }

    fn as_value(&self) -> serde_json::Value {
        match self {
            Self::Number(value) => json!(value),
            Self::Text(value) => json!(value),
            Self::Missing => json!(null),
        }
    }
}

impl<'de> Deserialize<'de> for ApiResCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        match value {
            serde_json::Value::Number(number) => number
                .as_i64()
                .map(Self::Number)
                .ok_or_else(|| serde::de::Error::custom("res_code number out of range")),
            serde_json::Value::String(text) => Ok(Self::Text(text)),
            serde_json::Value::Null => Ok(Self::Missing),
            _ => Err(serde::de::Error::custom("unexpected res_code value")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_share_info_sync_result(
        body: &str,
        share_code: &str,
        access_code: Option<&str>,
    ) -> CheckResult {
        let response: ShareInfoResponse = serde_json::from_str(body).unwrap();
        let mut metadata = base_metadata(share_code, access_code);
        metadata.insert("api_res_code".to_string(), json!(response.res_code.as_value()));
        metadata.insert(
            "api_res_code_label".to_string(),
            json!(res_code_label(&response.res_code)),
        );

        if let Some(need_access_code) = response.need_access_code {
            metadata.insert("need_access_code".to_string(), json!(need_access_code));
        }

        let normalized_url = normalize_share_url(share_code, access_code);

        match classify_share_info(&response, access_code.is_some()) {
            ShareInfoDecision::Return(status, reason) => CheckResult::new(
                format!("https://cloud.189.cn/t/{share_code}"),
                normalized_url,
                status,
                Provider::Pan189,
                reason,
                metadata,
            ),
            ShareInfoDecision::VerifyAccessCode => {
                panic!("expected synchronous share info classification")
            }
        }
    }

    fn build_access_code_result(body: &str, share_code: &str, access_code: &str) -> CheckResult {
        let response: CheckAccessCodeResponse = serde_json::from_str(body).unwrap();
        let metadata = base_metadata(share_code, Some(access_code));

        let (status, reason) = classify_access_code(&response);

        CheckResult::new(
            format!("https://cloud.189.cn/t/{share_code}?accessCode={access_code}"),
            normalize_share_url(share_code, Some(access_code)),
            status,
            Provider::Pan189,
            reason,
            metadata,
        )
    }

    #[test]
    fn extracts_share_code_from_short_path() {
        let url = Url::parse("https://cloud.189.cn/t/yYvIvyVfY7rm?accessCode=1hit").unwrap();

        assert_eq!(extract_share_code(&url).as_deref(), Some("yYvIvyVfY7rm"));
    }

    #[test]
    fn extracts_share_code_from_web_share_query() {
        let url =
            Url::parse("https://cloud.189.cn/web/share?code=nIB7Fr6Nn2ua").unwrap();

        assert_eq!(extract_share_code(&url).as_deref(), Some("nIB7Fr6Nn2ua"));
    }

    #[test]
    fn extracts_access_code_from_supported_query_names() {
        let url = Url::parse("https://cloud.189.cn/t/example?password=1hit").unwrap();

        assert_eq!(extract_access_code(&url).as_deref(), Some("1hit"));
    }

    #[test]
    fn extracts_share_code_and_access_code_from_embedded_web_share_query() {
        let url = Url::parse(
            "https://cloud.189.cn/web/share?code=UreieiIZJbU3%EF%BC%88%E8%AE%BF%E9%97%AE%E7%A0%81%EF%BC%9Axw6v%EF%BC%89",
        )
        .unwrap();

        assert_eq!(extract_share_code(&url).as_deref(), Some("UreieiIZJbU3"));
        assert_eq!(extract_access_code(&url).as_deref(), Some("xw6v"));
    }

    #[test]
    fn normalizes_embedded_web_share_url_to_short_form() {
        let url = Url::parse(
            "https://cloud.189.cn/web/share?code=UreieiIZJbU3%EF%BC%88%E8%AE%BF%E9%97%AE%E7%A0%81%EF%BC%9Axw6v%EF%BC%89",
        )
        .unwrap();

        assert_eq!(
            normalize_share_url(
                &extract_share_code(&url).unwrap(),
                extract_access_code(&url).as_deref(),
            ),
            "https://cloud.189.cn/t/UreieiIZJbU3?accessCode=xw6v"
        );
    }

    #[test]
    fn normalizes_web_share_url_to_short_form() {
        let normalized = normalize_share_url("yYvIvyVfY7rm", Some("1hit"));

        assert_eq!(
            normalized,
            "https://cloud.189.cn/t/yYvIvyVfY7rm?accessCode=1hit"
        );
    }

    #[test]
    fn marks_cancelled_share_as_invalid() {
        let result = build_share_info_sync_result(
            r#"{
                "res_code": "ShareNotFound",
                "res_message": "share not found or invalid."
            }"#,
            "nIB7Fr6Nn2ua",
            Some("sbl6"),
        );

        assert_eq!(result.status, CheckStatus::Invalid);
        assert_eq!(result.reason, "share_not_found");
        assert_eq!(result.metadata["api_res_code_label"], json!("share_not_found"));
    }

    #[test]
    fn marks_harmonized_share_as_invalid() {
        let result = build_share_info_sync_result(
            r#"{
                "res_code": "ShareAuditNotPass",
                "res_message": "share audit not pass."
            }"#,
            "uQ7vMzbMZvQn",
            None,
        );

        assert_eq!(result.status, CheckStatus::Invalid);
        assert_eq!(result.reason, "share_audit_blocked");
        assert_eq!(
            result.metadata["api_res_code_label"],
            json!("share_audit_blocked")
        );
    }

    #[test]
    fn marks_missing_access_code_as_invalid() {
        let result = build_share_info_sync_result(
            r#"{
                "res_code": 0,
                "res_message": "成功",
                "needAccessCode": 1,
                "reviewStatus": 1,
                "fileName": "示例分享"
            }"#,
            "yYvIvyVfY7rm",
            None,
        );

        assert_eq!(result.status, CheckStatus::Invalid);
        assert_eq!(result.reason, "missing_access_code");
        assert_eq!(result.metadata["need_access_code"], json!(1));
    }

    #[test]
    fn marks_password_protected_share_for_verification() {
        let decision = classify_share_info(
            &serde_json::from_str::<ShareInfoResponse>(
                r#"{
                    "res_code": 0,
                    "needAccessCode": 1,
                    "reviewStatus": 1
                }"#,
            )
            .unwrap(),
            true,
        );

        assert_eq!(decision, ShareInfoDecision::VerifyAccessCode);
    }

    #[test]
    fn marks_valid_access_code_as_valid() {
        let result = build_access_code_result(
            r#"{
                "res_code": 0,
                "res_message": "成功",
                "shareId": 12403121104649
            }"#,
            "yYvIvyVfY7rm",
            "1hit",
        );

        assert_eq!(result.status, CheckStatus::Valid);
        assert_eq!(result.reason, "share_available");
    }

    #[test]
    fn marks_invalid_access_code_as_invalid() {
        let result = build_access_code_result(
            r#"{
                "res_code": 0,
                "res_message": "成功",
                "success": false
            }"#,
            "yYvIvyVfY7rm",
            "wrong",
        );

        assert_eq!(result.status, CheckStatus::Invalid);
        assert_eq!(result.reason, "invalid_access_code");
    }

    #[test]
    fn marks_public_share_as_valid() {
        let result = build_share_info_sync_result(
            r#"{
                "res_code": 0,
                "res_message": "成功",
                "needAccessCode": 0,
                "reviewStatus": 1,
                "fileName": "公开分享"
            }"#,
            "publicShare",
            None,
        );

        assert_eq!(result.status, CheckStatus::Valid);
        assert_eq!(result.reason, "share_available");
    }
}
