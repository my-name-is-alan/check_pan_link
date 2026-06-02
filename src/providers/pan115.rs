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

const SHARE_SNAP_ENDPOINT: &str = "https://webapi.115.com/share/snap";
const ERRNO_INVALID_RECEIVE_CODE: i64 = 4_100_008;
const ERRNO_MISSING_RECEIVE_CODE: i64 = 4_100_012;
const ERRNO_INVALID_SHARE_CODE: i64 = 990_002;

pub struct Pan115Checker;

#[async_trait]
impl ProviderChecker for Pan115Checker {
    fn provider(&self) -> Provider {
        Provider::Pan115
    }

    fn matches(&self, url: &Url) -> bool {
        ["115.com", "115cdn.com", "anxia.com"]
            .iter()
            .any(|domain| host_is_or_subdomain(url, domain))
    }

    async fn check(&self, context: CheckContext) -> Result<CheckResult, CheckError> {
        let Some(share_code) = extract_share_code(&context.url) else {
            return basic_http_check(context, self.provider()).await;
        };

        Ok(check_share_snap(context, share_code, self.provider()).await)
    }
}

async fn check_share_snap(
    context: CheckContext,
    share_code: String,
    provider: Provider,
) -> CheckResult {
    let receive_code = extract_receive_code(&context.url);
    let original_url = context.original_url;
    let normalized_url = context.url.to_string();
    let mut metadata = base_metadata(&share_code, receive_code.as_deref());
    metadata.insert("api_endpoint".to_string(), json!(SHARE_SNAP_ENDPOINT));

    let mut query_params = vec![
        ("share_code", share_code.as_str()),
        ("cid", "0"),
        ("offset", "0"),
        ("limit", "1"),
        ("format", "json"),
    ];

    if let Some(receive_code) = receive_code.as_deref() {
        query_params.push(("receive_code", receive_code));
    }

    let request_url = Url::parse_with_params(SHARE_SNAP_ENDPOINT, &query_params)
        .expect("share snap endpoint should always produce a valid URL");
    let request = context.client.get(request_url);

    match request.send().await {
        Ok(response) => {
            metadata.insert(
                "api_http_status".to_string(),
                json!(response.status().as_u16()),
            );

            match response.json::<ShareSnapResponse>().await {
                Ok(body) => {
                    build_share_snap_result(original_url, normalized_url, provider, metadata, body)
                }
                Err(error) => {
                    metadata.insert("error".to_string(), json!(error.to_string()));
                    CheckResult::new(
                        original_url,
                        normalized_url,
                        CheckStatus::Unknown,
                        provider,
                        "share_snap_parse_failed",
                        metadata,
                    )
                }
            }
        }
        Err(error) => {
            let reason = if error.is_timeout() {
                "share_snap_timeout"
            } else if error.is_connect() {
                "share_snap_connection_failed"
            } else {
                "share_snap_request_failed"
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

fn build_share_snap_result(
    original_url: String,
    normalized_url: String,
    provider: Provider,
    mut metadata: BTreeMap<String, serde_json::Value>,
    body: ShareSnapResponse,
) -> CheckResult {
    metadata.insert("api_state".to_string(), json!(body.state));
    metadata.insert("api_errno".to_string(), json!(body.errno));
    metadata.insert(
        "page_entry_errno_name".to_string(),
        json!(page_entry_errno_name(body.errno)),
    );
    metadata.insert(
        "page_entry_error_kind".to_string(),
        json!(page_entry_error_kind(body.errno)),
    );

    if !body.errtype.is_empty() {
        metadata.insert("api_errtype".to_string(), json!(body.errtype));
    }

    if !body.error.is_empty() {
        metadata.insert("api_error".to_string(), json!(body.error));
        metadata.insert("page_entry_error_message".to_string(), json!(body.error));
    }

    if let Some(data) = body.data.as_ref() {
        if let Some(is_access) = data.is_access {
            metadata.insert("is_access".to_string(), json!(is_access));
        }

        if let Some(share_info) = data.shareinfo.as_ref() {
            if !share_info.share_title.is_empty() {
                metadata.insert("share_title".to_string(), json!(share_info.share_title));
            }

            if !share_info.forbid_reason.is_empty() {
                metadata.insert("forbid_reason".to_string(), json!(share_info.forbid_reason));
            }

            metadata.insert(
                "has_receive_code".to_string(),
                json!(share_info.has_receive_code),
            );
            metadata.insert("have_vio_file".to_string(), json!(share_info.have_vio_file));
            metadata.insert(
                "share_duration".to_string(),
                json!(share_info.share_duration),
            );
            metadata.insert("expire_time".to_string(), json!(share_info.expire_time));
            metadata.insert("file_size".to_string(), json!(share_info.file_size));
        }

        if let Some(share_state) = data.share_state() {
            metadata.insert("share_state".to_string(), json!(share_state));
            metadata.insert(
                "share_state_label".to_string(),
                json!(share_state_label(share_state)),
            );
        }
    }

    let (status, reason) = classify_share_snap(&body);

    CheckResult::new(
        original_url,
        normalized_url,
        status,
        provider,
        reason,
        metadata,
    )
}

fn classify_share_snap(body: &ShareSnapResponse) -> (CheckStatus, &'static str) {
    if !body.state {
        return match body.errno {
            ERRNO_MISSING_RECEIVE_CODE => (CheckStatus::Invalid, "missing_receive_code"),
            ERRNO_INVALID_RECEIVE_CODE => (CheckStatus::Invalid, "invalid_receive_code"),
            ERRNO_INVALID_SHARE_CODE => (CheckStatus::Invalid, "invalid_share_code"),
            _ => (CheckStatus::Unknown, "share_snap_api_error"),
        };
    }

    if body
        .data
        .as_ref()
        .and_then(|data| data.shareinfo.as_ref())
        .is_some_and(|shareinfo| shareinfo.share_state == 1 && shareinfo.have_vio_file > 0)
    {
        return (CheckStatus::Invalid, "share_contains_violation");
    }

    match body.data.as_ref().and_then(ShareSnapData::share_state) {
        Some(1) => (CheckStatus::Valid, "share_available"),
        Some(0) => (CheckStatus::Processing, "share_processing"),
        Some(2) => (CheckStatus::Invalid, "share_copyright_blocked"),
        Some(3) => (CheckStatus::Invalid, "share_pornography_blocked"),
        Some(4) => (CheckStatus::Invalid, "share_cancelled"),
        Some(5) => (CheckStatus::Invalid, "share_deleted"),
        Some(6) => (CheckStatus::Invalid, "share_violence_blocked"),
        Some(7) => (CheckStatus::Invalid, "share_expired"),
        Some(8) => (CheckStatus::Processing, "share_reviewing"),
        Some(_) => (CheckStatus::Unknown, "share_state_unknown"),
        None => (CheckStatus::Unknown, "share_state_missing"),
    }
}

fn base_metadata(
    share_code: &str,
    receive_code: Option<&str>,
) -> BTreeMap<String, serde_json::Value> {
    let mut metadata = BTreeMap::new();
    metadata.insert("share_code".to_string(), json!(share_code));
    metadata.insert(
        "receive_code_provided".to_string(),
        json!(receive_code.is_some()),
    );

    if let Some(receive_code) = receive_code {
        metadata.insert("receive_code".to_string(), json!(receive_code));
    }

    metadata
}

fn extract_share_code(url: &Url) -> Option<String> {
    let mut segments = url.path_segments()?;
    match (segments.next(), segments.next()) {
        (Some("s"), Some(share_code)) if !share_code.is_empty() => Some(share_code.to_string()),
        _ => None,
    }
}

fn extract_receive_code(url: &Url) -> Option<String> {
    url.query_pairs()
        .find(|(key, value)| {
            matches!(key.as_ref(), "password" | "receive_code") && !value.is_empty()
        })
        .map(|(_, value)| value.into_owned())
}

fn share_state_label(share_state: i64) -> &'static str {
    match share_state {
        0 => "processing",
        1 => "normal",
        2 => "copyright",
        3 => "pornography",
        4 => "cancelled",
        5 => "deleted",
        6 => "violence",
        7 => "expired",
        8 => "reviewing",
        _ => "unknown",
    }
}

fn page_entry_errno_name(errno: i64) -> &'static str {
    match errno {
        0 => "ok",
        ERRNO_MISSING_RECEIVE_CODE => "receive_code_required",
        ERRNO_INVALID_RECEIVE_CODE => "receive_code_invalid",
        ERRNO_INVALID_SHARE_CODE => "share_code_invalid",
        _ => "unknown_entry_error",
    }
}

fn page_entry_error_kind(errno: i64) -> &'static str {
    match errno {
        0 => "none",
        ERRNO_MISSING_RECEIVE_CODE | ERRNO_INVALID_RECEIVE_CODE => "receive_code",
        ERRNO_INVALID_SHARE_CODE => "share_code",
        _ => "unknown",
    }
}

#[derive(Debug, Deserialize)]
struct ShareSnapResponse {
    state: bool,
    #[serde(default)]
    error: String,
    errno: i64,
    #[serde(default)]
    errtype: String,
    #[serde(default)]
    data: Option<ShareSnapData>,
}

#[derive(Debug, Deserialize)]
struct ShareSnapData {
    #[serde(default)]
    is_access: Option<i64>,
    #[serde(default)]
    share_state: Option<i64>,
    #[serde(default)]
    shareinfo: Option<ShareInfo>,
}

impl ShareSnapData {
    fn share_state(&self) -> Option<i64> {
        self.share_state.or_else(|| {
            self.shareinfo
                .as_ref()
                .map(|shareinfo| shareinfo.share_state)
        })
    }
}

#[derive(Debug, Deserialize)]
struct ShareInfo {
    share_state: i64,
    #[serde(default)]
    share_title: String,
    #[serde(default)]
    forbid_reason: String,
    #[serde(default)]
    file_size: u64,
    #[serde(default)]
    has_receive_code: i64,
    #[serde(default)]
    have_vio_file: i64,
    #[serde(default)]
    share_duration: i64,
    #[serde(default)]
    expire_time: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_result(body: &str) -> CheckResult {
        let response: ShareSnapResponse = serde_json::from_str(body).unwrap();
        let metadata = base_metadata("swfsfjg3h7i", Some("l3a6"));

        build_share_snap_result(
            "https://115cdn.com/s/swfsfjg3h7i?password=l3a6".to_string(),
            "https://115cdn.com/s/swfsfjg3h7i?password=l3a6".to_string(),
            Provider::Pan115,
            metadata,
            response,
        )
    }

    #[test]
    fn extracts_share_code_from_share_path() {
        let url = Url::parse("https://115cdn.com/s/swfsfjg3h7i?password=l3a6").unwrap();
        assert_eq!(extract_share_code(&url).as_deref(), Some("swfsfjg3h7i"));
    }

    #[test]
    fn matches_anxia_share_domain() {
        let checker = Pan115Checker;
        let url = Url::parse("https://anxia.com/s/swh5dd83nwq?password=6969#").unwrap();

        assert!(checker.matches(&url));
    }

    #[test]
    fn extracts_receive_code_from_password_param() {
        let url = Url::parse("https://115cdn.com/s/swfsfjg3h7i?password=l3a6").unwrap();
        assert_eq!(extract_receive_code(&url).as_deref(), Some("l3a6"));
    }

    #[test]
    fn marks_available_share_as_valid() {
        let result = build_result(
            r#"{
                "state": true,
                "error": "",
                "errno": 0,
                "data": {
                    "shareinfo": {
                        "share_state": 1,
                        "share_title": "记录的地平线",
                        "forbid_reason": "",
                        "file_size": 1,
                        "has_receive_code": 1,
                        "have_vio_file": 0,
                        "share_duration": -1,
                        "expire_time": -1
                    }
                }
            }"#,
        );

        assert_eq!(result.status, CheckStatus::Valid);
        assert_eq!(result.reason, "share_available");
        assert_eq!(result.metadata["share_state"], json!(1));
    }

    #[test]
    fn marks_expired_share_as_invalid() {
        let result = build_result(
            r#"{
                "state": true,
                "error": "",
                "errno": 0,
                "data": {
                    "shareinfo": {
                        "share_state": 7,
                        "share_title": "伊波拉病毒.mkv",
                        "forbid_reason": "链接已过期",
                        "file_size": 62561206330,
                        "has_receive_code": 1,
                        "have_vio_file": 0,
                        "share_duration": 15,
                        "expire_time": 1773543266
                    }
                }
            }"#,
        );

        assert_eq!(result.status, CheckStatus::Invalid);
        assert_eq!(result.reason, "share_expired");
        assert_eq!(result.metadata["share_state_label"], json!("expired"));
        assert_eq!(result.metadata["forbid_reason"], json!("链接已过期"));
    }

    #[test]
    fn serializes_reviewing_share_status_as_processing() {
        let result = build_result(
            r#"{
                "state": true,
                "error": "",
                "errno": 0,
                "data": {
                    "shareinfo": {
                        "share_state": 8,
                        "share_title": "审核中的分享",
                        "forbid_reason": "",
                        "file_size": 1,
                        "has_receive_code": 1,
                        "have_vio_file": 0,
                        "share_duration": -1,
                        "expire_time": -1
                    }
                }
            }"#,
        );

        let json = serde_json::to_value(&result).unwrap();

        assert_eq!(result.reason, "share_reviewing");
        assert_eq!(result.metadata["share_state_label"], json!("reviewing"));
        assert_eq!(json["status"], "processing");
    }

    #[test]
    fn serializes_processing_share_status_as_processing() {
        let result = build_result(
            r#"{
                "state": true,
                "error": "",
                "errno": 0,
                "data": {
                    "shareinfo": {
                        "share_state": 0,
                        "share_title": "处理中分享",
                        "forbid_reason": "",
                        "file_size": 1,
                        "has_receive_code": 1,
                        "have_vio_file": 0,
                        "share_duration": -1,
                        "expire_time": -1
                    }
                }
            }"#,
        );

        let json = serde_json::to_value(&result).unwrap();

        assert_eq!(result.reason, "share_processing");
        assert_eq!(result.metadata["share_state_label"], json!("processing"));
        assert_eq!(json["status"], "processing");
    }

    #[test]
    fn marks_missing_receive_code_as_invalid() {
        let result = build_result(
            r#"{
                "state": false,
                "error": "请输入访问码",
                "errno": 4100012,
                "data": {
                    "is_access": 0
                }
            }"#,
        );

        assert_eq!(result.status, CheckStatus::Invalid);
        assert_eq!(result.reason, "missing_receive_code");
        assert_eq!(result.metadata["api_errno"], json!(4100012));
        assert_eq!(
            result.metadata["page_entry_errno_name"],
            json!("receive_code_required")
        );
        assert_eq!(
            result.metadata["page_entry_error_kind"],
            json!("receive_code")
        );
        assert_eq!(result.metadata["is_access"], json!(0));
    }

    #[test]
    fn marks_invalid_receive_code_as_invalid() {
        let result = build_result(
            r#"{
                "state": false,
                "error": "访问码错误",
                "errno": 4100008
            }"#,
        );

        assert_eq!(result.status, CheckStatus::Invalid);
        assert_eq!(result.reason, "invalid_receive_code");
        assert_eq!(
            result.metadata["page_entry_errno_name"],
            json!("receive_code_invalid")
        );
        assert_eq!(
            result.metadata["page_entry_error_kind"],
            json!("receive_code")
        );
    }

    #[test]
    fn marks_invalid_share_code_as_invalid() {
        let result = build_result(
            r#"{
                "state": false,
                "error": "参数错误。",
                "errno": 990002
            }"#,
        );

        assert_eq!(result.status, CheckStatus::Invalid);
        assert_eq!(result.reason, "invalid_share_code");
        assert_eq!(
            result.metadata["page_entry_errno_name"],
            json!("share_code_invalid")
        );
        assert_eq!(
            result.metadata["page_entry_error_kind"],
            json!("share_code")
        );
    }

    #[test]
    fn marks_share_with_violation_flag_as_invalid() {
        let result = build_result(
            r#"{
                "state": true,
                "error": "",
                "errno": 0,
                "data": {
                    "shareinfo": {
                        "share_state": 1,
                        "share_title": "丑陋的美国人-2010-[tmdb=32351]",
                        "forbid_reason": "",
                        "file_size": 3351054442,
                        "has_receive_code": 1,
                        "have_vio_file": 1,
                        "share_duration": -1,
                        "expire_time": -1
                    }
                }
            }"#,
        );

        assert_eq!(result.status, CheckStatus::Invalid);
        assert_eq!(result.reason, "share_contains_violation");
        assert_eq!(result.metadata["have_vio_file"], json!(1));
    }
}
