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

const API_BASE_URL: &str = "https://www.123pan.cn";
const SHARE_INFO_PATH: &str = "/gsb/s";
const RECEIVE_CODE_VERIFY_PATH: &str = "/gsb/s/share-list";
const ERRNO_INVALID_RECEIVE_CODE: i64 = 5_103;
const ERRNO_SHARE_LINK_INACTIVE: i64 = 5_104;

pub struct Pan123Checker;

#[async_trait]
impl ProviderChecker for Pan123Checker {
    fn provider(&self) -> Provider {
        Provider::Pan123
    }

    fn matches(&self, url: &Url) -> bool {
        [
            "123pan.com",
            "123pan.cn",
            "123865.com",
            "123912.com",
            "123684.com",
            "123635.com",
        ]
        .iter()
        .any(|domain| host_is_or_subdomain(url, domain))
    }

    async fn check(&self, context: CheckContext) -> Result<CheckResult, CheckError> {
        let Some(share_key) = extract_share_key(&context.url) else {
            return basic_http_check(context, self.provider()).await;
        };

        Ok(check_share(context, share_key, self.provider()).await)
    }
}

async fn check_share(context: CheckContext, share_key: String, provider: Provider) -> CheckResult {
    let CheckContext {
        original_url,
        url,
        client,
    } = context;

    let normalized_url = url.to_string();
    let receive_code = extract_receive_code(&url);
    let endpoints = endpoint_candidates(&url);
    if endpoints.is_empty() {
        return CheckResult::new(
            original_url,
            normalized_url,
            CheckStatus::Unknown,
            provider,
            "api_endpoint_build_failed",
            base_metadata(&share_key, receive_code.as_deref()),
        );
    }

    let mut last_result = None;
    for endpoints in endpoints {
        let result = check_share_with_endpoints(
            client.clone(),
            original_url.clone(),
            normalized_url.clone(),
            provider,
            &share_key,
            receive_code.clone(),
            endpoints,
        )
        .await;

        if !should_try_next_endpoint(&result) {
            return result;
        }

        last_result = Some(result);
    }

    last_result.unwrap_or_else(|| {
        CheckResult::new(
            original_url,
            normalized_url,
            CheckStatus::Unknown,
            provider,
            "api_endpoint_build_failed",
            base_metadata(&share_key, receive_code.as_deref()),
        )
    })
}

async fn check_share_with_endpoints(
    client: reqwest::Client,
    original_url: String,
    normalized_url: String,
    provider: Provider,
    share_key: &str,
    receive_code: Option<String>,
    endpoints: Pan123ApiEndpoints,
) -> CheckResult {
    let mut metadata = base_metadata(share_key, receive_code.as_deref());
    let share_info_endpoint = endpoints.share_info_endpoint;
    let receive_code_verify_endpoint = endpoints.receive_code_verify_endpoint;
    let share_info_url = format!("{share_info_endpoint}/{share_key}");
    metadata.insert("share_info_endpoint".to_string(), json!(share_info_url));
    metadata.insert(
        "receive_code_verify_endpoint".to_string(),
        json!(receive_code_verify_endpoint),
    );

    match client.get(&share_info_url).send().await {
        Ok(response) => {
            metadata.insert(
                "share_info_http_status".to_string(),
                json!(response.status().as_u16()),
            );

            match response.json::<Pan123ShareInfoResponse>().await {
                Ok(body) => {
                    build_share_info_result(
                        client,
                        original_url,
                        normalized_url,
                        provider,
                        metadata,
                        receive_code,
                        receive_code_verify_endpoint,
                        body,
                    )
                    .await
                }
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

fn should_try_next_endpoint(result: &CheckResult) -> bool {
    result.status == CheckStatus::Unknown
        && matches!(
            result.reason.as_str(),
            "share_info_timeout"
                | "share_info_connection_failed"
                | "share_info_request_failed"
                | "share_info_parse_failed"
                | "receive_code_verify_timeout"
                | "receive_code_verify_connection_failed"
                | "receive_code_verify_request_failed"
                | "receive_code_verify_parse_failed"
        )
}

async fn build_share_info_result(
    client: reqwest::Client,
    original_url: String,
    normalized_url: String,
    provider: Provider,
    mut metadata: BTreeMap<String, serde_json::Value>,
    receive_code: Option<String>,
    receive_code_verify_endpoint: String,
    body: Pan123ShareInfoResponse,
) -> CheckResult {
    metadata.insert("share_info_code".to_string(), json!(body.info.code));

    if !body.info.message.is_empty() {
        metadata.insert("share_info_message".to_string(), json!(body.info.message));
    }

    if let Some(data) = body.info.data.as_ref() {
        if let Some(user_id) = data.user_id {
            metadata.insert("share_user_id".to_string(), json!(user_id));
        }

        metadata.insert("share_has_pwd".to_string(), json!(data.has_pwd));
        metadata.insert("share_expired".to_string(), json!(data.expired));

        if !data.share_name.is_empty() {
            metadata.insert("share_name".to_string(), json!(data.share_name));
        }

        if !data.share_key.is_empty() {
            metadata.insert("share_key_from_api".to_string(), json!(data.share_key));
        }
    }

    match classify_share_info(&body, receive_code.is_some()) {
        ShareInfoDecision::Return(status, reason) => CheckResult::new(
            original_url,
            normalized_url,
            status,
            provider,
            reason,
            metadata,
        ),
        ShareInfoDecision::VerifyReceiveCode => {
            verify_receive_code(
                client,
                original_url,
                normalized_url,
                provider,
                metadata,
                receive_code.expect("receive code presence already checked"),
                &receive_code_verify_endpoint,
                body.info
                    .data
                    .as_ref()
                    .and_then(|data| {
                        if data.share_key.is_empty() {
                            None
                        } else {
                            Some(data.share_key.as_str())
                        }
                    })
                    .unwrap_or_default(),
            )
            .await
        }
    }
}

async fn verify_receive_code(
    client: reqwest::Client,
    original_url: String,
    normalized_url: String,
    provider: Provider,
    mut metadata: BTreeMap<String, serde_json::Value>,
    receive_code: String,
    receive_code_verify_endpoint: &str,
    share_key_from_api: &str,
) -> CheckResult {
    let share_key = metadata
        .get("share_code")
        .and_then(|value| value.as_str())
        .unwrap_or(share_key_from_api);

    let request_url = Url::parse_with_params(
        receive_code_verify_endpoint,
        &[("shareKey", share_key), ("SharePwd", receive_code.as_str())],
    )
    .expect("pan123 share list endpoint should always be valid");

    match client.get(request_url).send().await {
        Ok(response) => {
            metadata.insert(
                "receive_code_verify_http_status".to_string(),
                json!(response.status().as_u16()),
            );

            match response.json::<Pan123ShareListResponse>().await {
                Ok(body) => {
                    metadata.insert("receive_code_verify_code".to_string(), json!(body.code));

                    if !body.message.is_empty() {
                        metadata.insert(
                            "receive_code_verify_message".to_string(),
                            json!(body.message),
                        );
                    }

                    if let Some(data) = body.data.as_ref() {
                        metadata.insert(
                            "receive_code_verify_expired".to_string(),
                            json!(data.expired),
                        );

                        if let Some(len) = data.len {
                            metadata.insert("share_file_count".to_string(), json!(len));
                        }
                    }

                    let (status, reason) = classify_share_list(&body);

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
                        "receive_code_verify_parse_failed",
                        metadata,
                    )
                }
            }
        }
        Err(error) => {
            let reason = if error.is_timeout() {
                "receive_code_verify_timeout"
            } else if error.is_connect() {
                "receive_code_verify_connection_failed"
            } else {
                "receive_code_verify_request_failed"
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

fn classify_share_info(
    body: &Pan123ShareInfoResponse,
    receive_code_provided: bool,
) -> ShareInfoDecision {
    if body.info.code == ERRNO_SHARE_LINK_INACTIVE {
        return ShareInfoDecision::Return(CheckStatus::Invalid, "share_link_inactive");
    }

    if body.info.code != 0 {
        return ShareInfoDecision::Return(CheckStatus::Unknown, "share_info_api_error");
    }

    let Some(data) = body.info.data.as_ref() else {
        return ShareInfoDecision::Return(CheckStatus::Unknown, "share_info_missing_data");
    };

    if data.expired {
        return ShareInfoDecision::Return(CheckStatus::Invalid, "share_link_inactive");
    }

    if data.has_pwd {
        if receive_code_provided {
            ShareInfoDecision::VerifyReceiveCode
        } else {
            ShareInfoDecision::Return(CheckStatus::Invalid, "missing_receive_code")
        }
    } else {
        ShareInfoDecision::Return(CheckStatus::Valid, "share_available")
    }
}

fn classify_share_list(body: &Pan123ShareListResponse) -> (CheckStatus, &'static str) {
    match body.code {
        0 => match body.data.as_ref() {
            Some(data) if data.expired => (CheckStatus::Invalid, "share_link_inactive"),
            Some(_) => (CheckStatus::Valid, "share_available"),
            None => (CheckStatus::Unknown, "share_list_missing_data"),
        },
        ERRNO_INVALID_RECEIVE_CODE => (CheckStatus::Invalid, "invalid_receive_code"),
        ERRNO_SHARE_LINK_INACTIVE => (CheckStatus::Invalid, "share_link_inactive"),
        _ => (CheckStatus::Unknown, "share_list_api_error"),
    }
}

fn base_metadata(
    share_key: &str,
    receive_code: Option<&str>,
) -> BTreeMap<String, serde_json::Value> {
    let mut metadata = BTreeMap::new();
    metadata.insert("share_code".to_string(), json!(share_key));
    metadata.insert(
        "receive_code_provided".to_string(),
        json!(receive_code.is_some()),
    );

    if let Some(receive_code) = receive_code {
        metadata.insert("receive_code".to_string(), json!(receive_code));
    }

    metadata
}

fn extract_share_key(url: &Url) -> Option<String> {
    let mut segments = url.path_segments()?;
    match (segments.next(), segments.next()) {
        (Some("s"), Some(share_key)) if !share_key.is_empty() => Some(share_key.to_string()),
        (Some("ps"), Some(share_key)) if !share_key.is_empty() => Some(share_key.to_string()),
        (Some("123pan"), Some(share_key)) if !share_key.is_empty() => Some(share_key.to_string()),
        _ => None,
    }
}

fn extract_receive_code(url: &Url) -> Option<String> {
    url.query_pairs()
        .find(|(key, value)| {
            matches!(key.as_ref(), "pwd" | "password" | "receive_code") && !value.is_empty()
        })
        .map(|(_, value)| value.into_owned())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pan123ApiEndpoints {
    share_info_endpoint: String,
    receive_code_verify_endpoint: String,
}

fn endpoint_candidates(url: &Url) -> Vec<Pan123ApiEndpoints> {
    let mut endpoints = Vec::new();
    push_endpoint_candidate(&mut endpoints, API_BASE_URL);

    if let Some(base_url) = build_base_url(url) {
        push_endpoint_candidate(&mut endpoints, &base_url);
    }

    endpoints
}

fn push_endpoint_candidate(endpoints: &mut Vec<Pan123ApiEndpoints>, base_url: &str) {
    let candidate = Pan123ApiEndpoints {
        share_info_endpoint: format_endpoint(base_url, SHARE_INFO_PATH),
        receive_code_verify_endpoint: format_endpoint(base_url, RECEIVE_CODE_VERIFY_PATH),
    };

    if !endpoints.contains(&candidate) {
        endpoints.push(candidate);
    }
}

fn build_base_url(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let scheme = url.scheme();
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();

    Some(format!("{scheme}://{host}{port}"))
}

fn format_endpoint(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

#[derive(Debug, PartialEq, Eq)]
enum ShareInfoDecision {
    Return(CheckStatus, &'static str),
    VerifyReceiveCode,
}

#[derive(Debug, Deserialize)]
struct Pan123ShareInfoResponse {
    info: Pan123ShareInfoPayload,
}

#[derive(Debug, Deserialize)]
struct Pan123ShareInfoPayload {
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: Option<Pan123ShareInfoData>,
}

#[derive(Debug, Deserialize)]
struct Pan123ShareInfoData {
    #[serde(rename = "UserID")]
    user_id: Option<u64>,
    #[serde(rename = "ShareName", default)]
    share_name: String,
    #[serde(rename = "HasPwd", default)]
    has_pwd: bool,
    #[serde(rename = "Expired", default)]
    expired: bool,
    #[serde(rename = "ShareKey", default)]
    share_key: String,
}

#[derive(Debug, Deserialize)]
struct Pan123ShareListResponse {
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: Option<Pan123ShareListData>,
}

#[derive(Debug, Deserialize)]
struct Pan123ShareListData {
    #[serde(rename = "Expired", default)]
    expired: bool,
    #[serde(rename = "Len")]
    len: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_legacy_123865_share_domain() {
        let checker = Pan123Checker;
        let url = Url::parse("https://www.123865.com/s/IpPUVv-gGKj?pwd=Ocat").unwrap();

        assert!(checker.matches(&url));
    }

    #[test]
    fn matches_user_owned_share_123pan_cn_domain() {
        let checker = Pan123Checker;
        let url =
            Url::parse("https://1813278387.share.123pan.cn/123pan/RWJUVv-rIry?pwd=6XEO").unwrap();

        assert!(checker.matches(&url));
    }

    #[test]
    fn extracts_share_key_from_legacy_and_final_paths() {
        let legacy = Url::parse("https://www.123865.com/s/IpPUVv-gGKj?pwd=Ocat").unwrap();
        let final_url =
            Url::parse("https://1813308069.share.123pan.cn/123pan/IpPUVv-gGKj?pwd=Ocat").unwrap();

        assert_eq!(extract_share_key(&legacy).as_deref(), Some("IpPUVv-gGKj"));
        assert_eq!(
            extract_share_key(&final_url).as_deref(),
            Some("IpPUVv-gGKj")
        );
    }

    #[test]
    fn extracts_receive_code_from_pwd_param() {
        let url = Url::parse("https://www.123865.com/s/IpPUVv-gGKj?pwd=Ocat").unwrap();

        assert_eq!(extract_receive_code(&url).as_deref(), Some("Ocat"));
    }

    #[test]
    fn builds_api_endpoint_candidates_with_canonical_host_first() {
        let url =
            Url::parse("https://1813278387.share.123pan.cn/123pan/RWJUVv-rIry?pwd=6XEO").unwrap();
        let endpoints = endpoint_candidates(&url);

        assert_eq!(endpoints.len(), 2);
        assert_eq!(
            endpoints[0].share_info_endpoint,
            "https://www.123pan.cn/gsb/s"
        );
        assert_eq!(
            endpoints[0].receive_code_verify_endpoint,
            "https://www.123pan.cn/gsb/s/share-list"
        );
        assert_eq!(
            endpoints[1].share_info_endpoint,
            "https://1813278387.share.123pan.cn/gsb/s"
        );
        assert_eq!(
            endpoints[1].receive_code_verify_endpoint,
            "https://1813278387.share.123pan.cn/gsb/s/share-list"
        );
    }

    #[test]
    fn endpoint_candidates_do_not_repeat_canonical_host() {
        let url = Url::parse("https://www.123pan.cn/s/RWJUVv-rIry?pwd=6XEO").unwrap();
        let endpoints = endpoint_candidates(&url);

        assert_eq!(endpoints.len(), 1);
        assert_eq!(
            endpoints[0].share_info_endpoint,
            "https://www.123pan.cn/gsb/s"
        );
    }

    #[test]
    fn classifies_inactive_share_info_as_invalid() {
        let body: Pan123ShareInfoResponse = serde_json::from_str(
            r#"{
                "info": {
                    "code": 5104,
                    "message": "分享链接已失效",
                    "data": null
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            classify_share_info(&body, false),
            ShareInfoDecision::Return(CheckStatus::Invalid, "share_link_inactive")
        );
    }

    #[test]
    fn classifies_missing_receive_code_as_invalid() {
        let body: Pan123ShareInfoResponse = serde_json::from_str(
            r#"{
                "info": {
                    "code": 0,
                    "message": "",
                    "data": {
                        "UserID": 1813308069,
                        "ShareName": "样例分享",
                        "HasPwd": true,
                        "Expired": false,
                        "ShareKey": "IpPUVv-gGKj"
                    }
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            classify_share_info(&body, false),
            ShareInfoDecision::Return(CheckStatus::Invalid, "missing_receive_code")
        );
    }

    #[test]
    fn classifies_pwd_protected_share_for_verification() {
        let body: Pan123ShareInfoResponse = serde_json::from_str(
            r#"{
                "info": {
                    "code": 0,
                    "message": "",
                    "data": {
                        "UserID": 1813308069,
                        "ShareName": "样例分享",
                        "HasPwd": true,
                        "Expired": false,
                        "ShareKey": "IpPUVv-gGKj"
                    }
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            classify_share_info(&body, true),
            ShareInfoDecision::VerifyReceiveCode
        );
    }

    #[test]
    fn classifies_invalid_receive_code_as_invalid() {
        let body: Pan123ShareListResponse = serde_json::from_str(
            r#"{
                "code": 5103,
                "message": "提取码错误",
                "data": null
            }"#,
        )
        .unwrap();

        assert_eq!(
            classify_share_list(&body),
            (CheckStatus::Invalid, "invalid_receive_code")
        );
    }

    #[test]
    fn classifies_successful_share_list_as_valid() {
        let body: Pan123ShareListResponse = serde_json::from_str(
            r#"{
                "code": 0,
                "message": "",
                "data": {
                    "Expired": false,
                    "Len": 1
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            classify_share_list(&body),
            (CheckStatus::Valid, "share_available")
        );
    }
}
