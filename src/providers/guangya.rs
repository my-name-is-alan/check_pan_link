use std::collections::BTreeMap;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use url::Url;

use crate::{
    checker::{CheckResult, CheckStatus, Provider},
    error::CheckError,
    providers::common::{CheckContext, ProviderChecker, basic_http_check, host_is_or_subdomain},
};

pub(crate) const SHARE_SUMMARY_ENDPOINT: &str =
    "https://api.guangyapan.com/userres/v1/get_share_summary";
pub(crate) const ACCESS_TOKEN_ENDPOINT: &str =
    "https://api.guangyapan.com/userres/v1/get_share_access_token";

pub(crate) const CODE_INVALID_SHARE_ID: i64 = 112;
pub(crate) const CODE_INVALID_SHARE: i64 = 200;
pub(crate) const CODE_INVALID_SHARE_ALT: i64 = 201;
pub(crate) const CODE_SHARE_EXPIRED: i64 = 202;
pub(crate) const CODE_INVALID_RECEIVE_CODE: i64 = 209;
pub(crate) const SHARE_STATUS_AVAILABLE: i64 = 1;
pub(crate) const SHARE_STATUS_EXPIRED: i64 = 2;

pub struct GuangyaPanChecker;

#[async_trait]
impl ProviderChecker for GuangyaPanChecker {
    fn provider(&self) -> Provider {
        Provider::GuangyaPan
    }

    fn matches(&self, url: &Url) -> bool {
        host_is_or_subdomain(url, "guangyapan.com")
    }

    async fn check(&self, context: CheckContext) -> Result<CheckResult, CheckError> {
        if extract_share_id(&context.url).is_none() {
            return basic_http_check(context, self.provider()).await;
        }

        check_share_with_endpoints(context, SHARE_SUMMARY_ENDPOINT, ACCESS_TOKEN_ENDPOINT).await
    }
}

pub(crate) async fn check_share_with_endpoints(
    context: CheckContext,
    summary_endpoint: &str,
    token_endpoint: &str,
) -> Result<CheckResult, CheckError> {
    let Some(share_id) = extract_share_id(&context.url) else {
        return basic_http_check(context, Provider::GuangyaPan).await;
    };

    let receive_code = extract_receive_code(&context.url);
    let original_url = context.original_url.clone();
    let normalized_url = context.url.to_string();
    let mut metadata = base_metadata(&share_id, receive_code.as_deref());
    metadata.insert(
        "share_summary_endpoint".to_string(),
        json!(summary_endpoint),
    );
    metadata.insert("access_token_endpoint".to_string(), json!(token_endpoint));

    let summary = match fetch_share_summary(&context.client, summary_endpoint, &share_id).await {
        Ok(summary) => summary,
        Err(error) => {
            metadata.insert("error".to_string(), json!(error.to_string()));
            return Ok(CheckResult::new(
                original_url,
                normalized_url,
                CheckStatus::Unknown,
                Provider::GuangyaPan,
                error.reason(),
                metadata,
            ));
        }
    };

    metadata.insert("share_summary_code".to_string(), json!(summary.code()));
    if !summary.msg.is_empty() {
        metadata.insert(
            "share_summary_message".to_string(),
            json!(summary.msg.clone()),
        );
    }

    let Some(summary_data) = classify_summary_for_check(&summary, &mut metadata) else {
        return Ok(CheckResult::new(
            original_url,
            normalized_url,
            summary_status(&summary),
            Provider::GuangyaPan,
            summary_reason(&summary),
            metadata,
        ));
    };

    add_summary_metadata(&mut metadata, &summary_data);

    if let Some(reason) = inactive_share_reason(summary_data.share_status) {
        return Ok(CheckResult::new(
            original_url,
            normalized_url,
            CheckStatus::Invalid,
            Provider::GuangyaPan,
            reason,
            metadata,
        ));
    }

    if summary_data.need_code && receive_code.is_none() {
        return Ok(CheckResult::new(
            original_url,
            normalized_url,
            CheckStatus::Invalid,
            Provider::GuangyaPan,
            "missing_receive_code",
            metadata,
        ));
    }

    let token = match fetch_access_token(
        &context.client,
        token_endpoint,
        &share_id,
        receive_code.as_deref(),
    )
    .await
    {
        Ok(token) => token,
        Err(error) => {
            metadata.insert("error".to_string(), json!(error.to_string()));
            return Ok(CheckResult::new(
                original_url,
                normalized_url,
                CheckStatus::Unknown,
                Provider::GuangyaPan,
                error.reason(),
                metadata,
            ));
        }
    };

    metadata.insert("access_token_code".to_string(), json!(token.code()));
    if !token.msg.is_empty() {
        metadata.insert("access_token_message".to_string(), json!(token.msg.clone()));
    }

    let (status, reason) = classify_access_token_for_check(&token);
    if status == CheckStatus::Valid {
        metadata.insert("access_token_verified".to_string(), json!(true));
    }

    Ok(CheckResult::new(
        original_url,
        normalized_url,
        status,
        Provider::GuangyaPan,
        reason,
        metadata,
    ))
}

fn base_metadata(
    share_id: &str,
    receive_code: Option<&str>,
) -> BTreeMap<String, serde_json::Value> {
    let mut metadata = BTreeMap::new();
    metadata.insert("share_id".to_string(), json!(share_id));
    metadata.insert(
        "receive_code_provided".to_string(),
        json!(receive_code.is_some()),
    );

    if let Some(receive_code) = receive_code {
        metadata.insert("receive_code".to_string(), json!(receive_code));
    }

    metadata
}

fn add_summary_metadata(
    metadata: &mut BTreeMap<String, serde_json::Value>,
    data: &ShareSummaryData,
) {
    metadata.insert("need_receive_code".to_string(), json!(data.need_code));

    if let Some(share_status) = data.share_status {
        metadata.insert("share_status".to_string(), json!(share_status));
        metadata.insert(
            "share_status_label".to_string(),
            json!(share_status_label(share_status)),
        );
        metadata.insert(
            "share_available".to_string(),
            json!(is_share_status_available(Some(share_status))),
        );
    }

    if let Some(left_time) = data.left_time {
        metadata.insert("left_time".to_string(), json!(left_time));
    }

    if let Some(title) = data.title.as_ref().filter(|title| !title.is_empty()) {
        metadata.insert("share_title".to_string(), json!(title));
    }

    if let Some(user_id) = data.user_id.as_ref().filter(|user_id| !user_id.is_empty()) {
        metadata.insert("share_user_id".to_string(), json!(user_id));
    }

    if let Some(nick_name) = data
        .nick_name
        .as_ref()
        .filter(|nick_name| !nick_name.is_empty())
    {
        metadata.insert("share_user_name".to_string(), json!(nick_name));
    }
}

pub(crate) async fn fetch_share_summary(
    client: &Client,
    endpoint: &str,
    share_id: &str,
) -> Result<GuangyaApiResponse<ShareSummaryData>, GuangyaRequestError> {
    post_guangya_json(client, endpoint, &ShareSummaryRequest { share_id }).await
}

pub(crate) async fn fetch_access_token(
    client: &Client,
    endpoint: &str,
    share_id: &str,
    receive_code: Option<&str>,
) -> Result<GuangyaApiResponse<AccessTokenData>, GuangyaRequestError> {
    post_guangya_json(
        client,
        endpoint,
        &AccessTokenRequest {
            share_id,
            code: receive_code,
        },
    )
    .await
}

pub(crate) async fn post_guangya_json<T, B>(
    client: &Client,
    endpoint: &str,
    body: &B,
) -> Result<GuangyaApiResponse<T>, GuangyaRequestError>
where
    T: for<'de> Deserialize<'de>,
    B: Serialize + ?Sized,
{
    let response = with_guangya_headers(client.post(endpoint))
        .json(body)
        .send()
        .await
        .map_err(GuangyaRequestError::Request)?;

    response
        .json::<GuangyaApiResponse<T>>()
        .await
        .map_err(GuangyaRequestError::Parse)
}

fn with_guangya_headers(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    request
        .header("dt", "4")
        .header("did", "check-pan-link")
        .header(
            "traceparent",
            "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01",
        )
        .header("Origin", "https://www.guangyapan.com")
        .header("Referer", "https://www.guangyapan.com/")
}

fn classify_summary_for_check(
    summary: &GuangyaApiResponse<ShareSummaryData>,
    metadata: &mut BTreeMap<String, serde_json::Value>,
) -> Option<ShareSummaryData> {
    match summary.code() {
        0 => summary.data.clone(),
        CODE_INVALID_SHARE_ID
        | CODE_INVALID_SHARE
        | CODE_INVALID_SHARE_ALT
        | CODE_SHARE_EXPIRED => None,
        code => {
            metadata.insert("unclassified_summary_code".to_string(), json!(code));
            None
        }
    }
}

fn summary_status(summary: &GuangyaApiResponse<ShareSummaryData>) -> CheckStatus {
    match summary.code() {
        CODE_INVALID_SHARE_ID
        | CODE_INVALID_SHARE
        | CODE_INVALID_SHARE_ALT
        | CODE_SHARE_EXPIRED => CheckStatus::Invalid,
        _ => CheckStatus::Unknown,
    }
}

fn summary_reason(summary: &GuangyaApiResponse<ShareSummaryData>) -> &'static str {
    match summary.code() {
        CODE_INVALID_SHARE_ID => "invalid_share_id",
        CODE_INVALID_SHARE | CODE_INVALID_SHARE_ALT => "invalid_share",
        CODE_SHARE_EXPIRED => "share_expired",
        0 => "share_summary_missing_data",
        _ => "share_summary_api_error",
    }
}

fn classify_access_token_for_check(
    token: &GuangyaApiResponse<AccessTokenData>,
) -> (CheckStatus, &'static str) {
    match token.code() {
        0 if token
            .data
            .as_ref()
            .is_some_and(|data| !data.access_token.is_empty()) =>
        {
            (CheckStatus::Valid, "share_available")
        }
        0 => (CheckStatus::Unknown, "access_token_missing_data"),
        CODE_INVALID_RECEIVE_CODE => (CheckStatus::Invalid, "invalid_receive_code"),
        CODE_INVALID_SHARE_ID => (CheckStatus::Invalid, "invalid_share_id"),
        _ => (CheckStatus::Unknown, "access_token_api_error"),
    }
}

pub(crate) fn inactive_share_reason(share_status: Option<i64>) -> Option<&'static str> {
    match share_status {
        Some(SHARE_STATUS_AVAILABLE) | None => None,
        Some(SHARE_STATUS_EXPIRED) => Some("share_expired"),
        Some(_) => Some("invalid_share"),
    }
}

pub(crate) fn is_share_status_available(share_status: Option<i64>) -> bool {
    inactive_share_reason(share_status).is_none()
}

pub(crate) fn share_status_label(share_status: i64) -> &'static str {
    match share_status {
        SHARE_STATUS_AVAILABLE => "active",
        SHARE_STATUS_EXPIRED => "expired",
        _ => "inactive",
    }
}

pub(crate) fn extract_share_id(url: &Url) -> Option<String> {
    let mut segments = url.path_segments()?;
    match (segments.next(), segments.next()) {
        (Some("s"), Some(share_id)) if !share_id.is_empty() => Some(share_id.to_string()),
        _ => None,
    }
}

pub(crate) fn extract_receive_code(url: &Url) -> Option<String> {
    url.query_pairs()
        .find(|(key, value)| key == "code" && !value.is_empty())
        .map(|(_, value)| value.into_owned())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShareSummaryRequest<'a> {
    share_id: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccessTokenRequest<'a> {
    share_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub(crate) struct GuangyaApiResponse<T> {
    #[serde(default)]
    pub(crate) code: Option<i64>,
    #[serde(default)]
    pub(crate) msg: String,
    #[serde(default)]
    pub(crate) data: Option<T>,
}

impl<T> GuangyaApiResponse<T> {
    pub(crate) fn code(&self) -> i64 {
        self.code.unwrap_or(0)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShareSummaryData {
    #[serde(default)]
    pub(crate) nick_name: Option<String>,
    #[serde(default)]
    pub(crate) left_time: Option<i64>,
    #[serde(default)]
    pub(crate) need_code: bool,
    #[serde(default)]
    pub(crate) user_id: Option<String>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) share_status: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccessTokenData {
    #[serde(default)]
    pub(crate) access_token: String,
}

#[derive(Debug)]
pub(crate) enum GuangyaRequestError {
    Request(reqwest::Error),
    Parse(reqwest::Error),
}

impl GuangyaRequestError {
    fn reason(&self) -> &'static str {
        match self {
            Self::Request(error) if error.is_timeout() => "guangya_request_timeout",
            Self::Request(error) if error.is_connect() => "guangya_connection_failed",
            Self::Request(_) => "guangya_request_failed",
            Self::Parse(_) => "guangya_response_parse_failed",
        }
    }
}

impl std::fmt::Display for GuangyaRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Request(error) | Self::Parse(error) => error.fmt(formatter),
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{Json, Router, extract::State, routing::post};
    use serde_json::json;
    use tokio::net::TcpListener;

    use super::*;
    use crate::checker::CheckStatus;

    #[test]
    fn matches_guangya_share_domains() {
        let checker = GuangyaPanChecker;

        assert!(
            checker.matches(
                &Url::parse("https://www.guangyapan.com/s/1910961250145955889_adz3Lo8EdLN_2BBy")
                    .unwrap()
            )
        );
        assert!(checker.matches(
            &Url::parse("https://guangyapan.com/s/1910961611262906426_adz3Lo8EdLN_2BBy").unwrap()
        ));
        assert!(
            !checker.matches(
                &Url::parse("https://evilguangyapan.com/s/1910961250145955889_adz3Lo8EdLN_2BBy")
                    .unwrap()
            )
        );
    }

    #[test]
    fn extracts_share_id_from_share_path() {
        let url = Url::parse("https://www.guangyapan.com/s/1910961250145955889_adz3Lo8EdLN_2BBy")
            .unwrap();

        assert_eq!(
            extract_share_id(&url).as_deref(),
            Some("1910961250145955889_adz3Lo8EdLN_2BBy")
        );
    }

    #[test]
    fn extracts_receive_code_from_code_param() {
        let url = Url::parse(
            "https://www.guangyapan.com/s/1910961611262906426_adz3Lo8EdLN_2BBy?code=bmiu",
        )
        .unwrap();

        assert_eq!(extract_receive_code(&url).as_deref(), Some("bmiu"));
    }

    #[tokio::test]
    async fn marks_missing_receive_code_from_share_summary_without_http_status_metadata() {
        let (summary_endpoint, token_endpoint) = spawn_share_api(
            json!({
                "msg": "success",
                "data": {
                    "needCode": true,
                    "shareStatus": 1,
                    "title": "需要提取码的分享",
                    "userId": "user-1",
                    "leftTime": -1
                }
            }),
            json!({"msg": "success", "data": {"accessToken": "token"}}),
        )
        .await;
        let client = build_test_client();
        let url = Url::parse("https://www.guangyapan.com/s/1910961611262906426_adz3Lo8EdLN_2BBy")
            .unwrap();

        let result = check_share_with_endpoints(
            CheckContext {
                original_url: url.to_string(),
                url,
                client,
            },
            &summary_endpoint,
            &token_endpoint,
        )
        .await
        .unwrap();

        assert_eq!(result.status, CheckStatus::Invalid);
        assert_eq!(result.reason, "missing_receive_code");
        assert_eq!(result.provider, Provider::GuangyaPan);
        assert!(!result.metadata.contains_key("http_status"));
        assert_eq!(result.metadata["need_receive_code"], json!(true));
        assert_eq!(result.metadata["share_title"], json!("需要提取码的分享"));
    }

    #[tokio::test]
    async fn verifies_receive_code_before_marking_share_available() {
        let (summary_endpoint, token_endpoint) = spawn_share_api(
            json!({
                "msg": "success",
                "data": {
                    "needCode": true,
                    "shareStatus": 1,
                    "title": "可用分享",
                    "userId": "user-1",
                    "leftTime": -1
                }
            }),
            json!({"msg": "success", "data": {"accessToken": "token"}}),
        )
        .await;
        let client = build_test_client();
        let url = Url::parse(
            "https://www.guangyapan.com/s/1910961611262906426_adz3Lo8EdLN_2BBy?code=bmiu",
        )
        .unwrap();

        let result = check_share_with_endpoints(
            CheckContext {
                original_url: url.to_string(),
                url,
                client,
            },
            &summary_endpoint,
            &token_endpoint,
        )
        .await
        .unwrap();

        assert_eq!(result.status, CheckStatus::Valid);
        assert_eq!(result.reason, "share_available");
        assert_eq!(
            result.metadata["share_id"],
            json!("1910961611262906426_adz3Lo8EdLN_2BBy")
        );
        assert_eq!(result.metadata["receive_code_provided"], json!(true));
        assert_eq!(result.metadata["receive_code"], json!("bmiu"));
        assert_eq!(result.metadata["access_token_verified"], json!(true));
    }

    #[tokio::test]
    async fn marks_invalid_receive_code_from_access_token_api() {
        let (summary_endpoint, token_endpoint) = spawn_share_api(
            json!({
                "msg": "success",
                "data": {
                    "needCode": true,
                    "shareStatus": 1,
                    "title": "可用分享"
                }
            }),
            json!({"code": 209, "msg": "提取码错误"}),
        )
        .await;
        let client = build_test_client();
        let url = Url::parse(
            "https://www.guangyapan.com/s/1910961611262906426_adz3Lo8EdLN_2BBy?code=bad",
        )
        .unwrap();

        let result = check_share_with_endpoints(
            CheckContext {
                original_url: url.to_string(),
                url,
                client,
            },
            &summary_endpoint,
            &token_endpoint,
        )
        .await
        .unwrap();

        assert_eq!(result.status, CheckStatus::Invalid);
        assert_eq!(result.reason, "invalid_receive_code");
        assert_eq!(result.metadata["access_token_code"], json!(209));
    }

    #[tokio::test]
    async fn marks_expired_share_status_without_fetching_access_token() {
        let (summary_endpoint, token_endpoint) = spawn_share_api(
            json!({
                "msg": "success",
                "data": {
                    "needCode": false,
                    "shareStatus": 2,
                    "title": "已过期分享",
                    "leftTime": 0
                }
            }),
            json!({"msg": "success", "data": {"accessToken": "token"}}),
        )
        .await;
        let client = build_test_client();
        let url = Url::parse("https://www.guangyapan.com/s/1910961250145955889_adz3Lo8EdLN_2BBy")
            .unwrap();

        let result = check_share_with_endpoints(
            CheckContext {
                original_url: url.to_string(),
                url,
                client,
            },
            &summary_endpoint,
            &token_endpoint,
        )
        .await
        .unwrap();

        assert_eq!(result.status, CheckStatus::Invalid);
        assert_eq!(result.reason, "share_expired");
        assert_eq!(result.metadata["share_status"], json!(2));
        assert_eq!(result.metadata["share_status_label"], json!("expired"));
        assert!(!result.metadata.contains_key("access_token_code"));
    }

    async fn spawn_share_api(
        summary_response: serde_json::Value,
        token_response: serde_json::Value,
    ) -> (String, String) {
        #[derive(Clone)]
        struct MockState {
            summary_response: serde_json::Value,
            token_response: serde_json::Value,
        }

        async fn summary(State(state): State<MockState>) -> Json<serde_json::Value> {
            Json(state.summary_response)
        }

        async fn token(State(state): State<MockState>) -> Json<serde_json::Value> {
            Json(state.token_response)
        }

        let app = Router::new()
            .route("/summary", post(summary))
            .route("/token", post(token))
            .with_state(MockState {
                summary_response,
                token_response,
            });

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        (
            format!("http://{address}/summary"),
            format!("http://{address}/token"),
        )
    }

    fn build_test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .no_proxy()
            .build()
            .unwrap()
    }
}
