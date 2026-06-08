use reqwest::Client;
use serde::Deserialize;
use url::Url;

use crate::error::ShareListError;

pub(crate) const SHARE_INFO_ENDPOINT: &str =
    "https://cloud.189.cn/api/open/share/getShareInfoByCodeV2.action";
pub(crate) const CHECK_ACCESS_CODE_ENDPOINT: &str =
    "https://cloud.189.cn/api/open/share/checkAccessCode.action";
pub(crate) const LIST_SHARE_DIR_ENDPOINT: &str =
    "https://cloud.189.cn/api/open/share/listShareDir.action";

pub(crate) const RES_CODE_SHARE_NOT_FOUND: &str = "ShareNotFound";
pub(crate) const RES_CODE_SHARE_AUDIT_NOT_PASS: &str = "ShareAuditNotPass";

#[derive(Debug, Clone)]
pub(crate) struct ShareSession {
    pub share_id: i64,
    pub share_mode: i64,
    pub file_id: String,
    pub is_folder: bool,
    pub file_name: String,
    pub access_code: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ShareInfoBody {
    #[serde(rename = "res_code", default)]
    pub res_code: ApiResCode,
    #[serde(rename = "res_message", default)]
    pub res_message: String,
    #[serde(rename = "needAccessCode", default)]
    pub need_access_code: Option<i64>,
    #[serde(rename = "fileName", default)]
    pub file_name: String,
    #[serde(rename = "fileId", default)]
    pub file_id: String,
    #[serde(rename = "isFolder", default)]
    pub is_folder: Option<bool>,
    #[serde(rename = "shareMode", default)]
    pub share_mode: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CheckAccessCodeBody {
    #[serde(rename = "res_code", default)]
    pub res_code: ApiResCode,
    #[serde(rename = "res_message", default)]
    pub res_message: String,
    #[serde(rename = "shareId", default)]
    pub share_id: Option<i64>,
    #[serde(default)]
    pub success: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListShareDirBody {
    #[serde(rename = "res_code", default)]
    pub res_code: ApiResCode,
    #[serde(rename = "res_message", default)]
    pub res_message: String,
    #[serde(rename = "fileListAO", default)]
    pub file_list: Option<FileListAo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FileListAo {
    #[serde(default)]
    pub count: Option<i64>,
    #[serde(rename = "fileList", default)]
    pub files: Vec<ShareFileEntry>,
    #[serde(rename = "folderList", default)]
    pub folders: Vec<ShareFolderEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct ShareFileEntry {
    #[serde(default, deserialize_with = "deserialize_id")]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(rename = "parentId", default, deserialize_with = "deserialize_id")]
    pub parent_id: String,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct ShareFolderEntry {
    #[serde(default, deserialize_with = "deserialize_id")]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "parentId", default, deserialize_with = "deserialize_id")]
    pub parent_id: String,
    #[serde(rename = "fileListSize", default)]
    pub file_list_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum ApiResCode {
    Number(i64),
    Text(String),
    #[default]
    Missing,
}

impl ApiResCode {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Number(0))
    }

    pub fn is_text(&self, expected: &str) -> bool {
        matches!(self, Self::Text(code) if code == expected)
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

pub(crate) async fn fetch_share_info(
    client: &Client,
    share_code: &str,
) -> Result<ShareInfoBody, ShareListError> {
    let request_url = Url::parse_with_params(
        SHARE_INFO_ENDPOINT,
        &[("returnType", "json"), ("shareCode", share_code)],
    )
    .expect("pan189 share info endpoint should always be valid");

    let response = with_share_headers(client.get(request_url), share_code)
        .send()
        .await
        .map_err(|error| ShareListError::RequestFailed(error.to_string()))?;

    response
        .json::<ShareInfoBody>()
        .await
        .map_err(|error| ShareListError::ParseFailed(error.to_string()))
}

pub(crate) async fn check_access_code(
    client: &Client,
    share_code: &str,
    access_code: &str,
) -> Result<CheckAccessCodeBody, ShareListError> {
    let request_url = Url::parse_with_params(
        CHECK_ACCESS_CODE_ENDPOINT,
        &[
            ("returnType", "json"),
            ("shareCode", share_code),
            ("accessCode", access_code),
        ],
    )
    .expect("pan189 access code endpoint should always be valid");

    let response = client
        .get(request_url)
        .header("Accept", "application/json;charset=UTF-8")
        .header("Referer", "https://cloud.189.cn/web/main/")
        .send()
        .await
        .map_err(|error| ShareListError::RequestFailed(error.to_string()))?;

    response
        .json::<CheckAccessCodeBody>()
        .await
        .map_err(|error| ShareListError::ParseFailed(error.to_string()))
}

pub(crate) async fn resolve_share_session(
    client: &Client,
    share_code: &str,
    access_code: Option<&str>,
) -> Result<ShareSession, ShareListError> {
    let info = fetch_share_info(client, share_code).await?;

    if info.res_code.is_text(RES_CODE_SHARE_NOT_FOUND) {
        return Err(ShareListError::InvalidShareCode);
    }

    if info.res_code.is_text(RES_CODE_SHARE_AUDIT_NOT_PASS) {
        return Err(ShareListError::Api("share audit not pass".to_string()));
    }

    if !info.res_code.is_success() {
        return Err(ShareListError::Api(info.res_message));
    }

    let needs_access_code = info.need_access_code.is_some_and(|value| value == 1);
    let resolved_access_code = if needs_access_code {
        access_code
            .filter(|value| !value.is_empty())
            .ok_or(ShareListError::MissingReceiveCode)?
            .to_string()
    } else {
        access_code.unwrap_or("").to_string()
    };

    let check = check_access_code(client, share_code, &resolved_access_code).await?;

    if check.res_code.is_text(RES_CODE_SHARE_NOT_FOUND) {
        return Err(ShareListError::InvalidShareCode);
    }

    if !check.res_code.is_success() {
        return Err(ShareListError::Api(check.res_message));
    }

    let Some(share_id) = check.share_id else {
        if check.success == Some(false) {
            return Err(ShareListError::InvalidReceiveCode);
        }

        return Err(ShareListError::Api(
            "failed to resolve share id for access code".to_string(),
        ));
    };

    Ok(ShareSession {
        share_id,
        share_mode: info.share_mode.unwrap_or(1),
        file_id: info.file_id,
        is_folder: info.is_folder.unwrap_or(true),
        file_name: info.file_name,
        access_code: resolved_access_code,
    })
}

pub(crate) async fn list_share_dir(
    client: &Client,
    session: &ShareSession,
    share_code: &str,
    folder_id: &str,
    page_num: i64,
    page_size: i64,
) -> Result<ListShareDirBody, ShareListError> {
    let request_url = Url::parse_with_params(
        LIST_SHARE_DIR_ENDPOINT,
        &[
            ("returnType", "json"),
            ("pageNum", &page_num.to_string()),
            ("pageSize", &page_size.to_string()),
            ("fileId", folder_id),
            ("shareDirFileId", folder_id),
            ("isFolder", if session.is_folder { "true" } else { "false" }),
            ("shareId", &session.share_id.to_string()),
            ("shareMode", &session.share_mode.to_string()),
            ("iconOption", "5"),
            ("orderBy", "lastOpTime"),
            ("descending", "true"),
            ("accessCode", &session.access_code),
        ],
    )
    .expect("pan189 list share dir endpoint should always be valid");

    let response = with_share_headers(client.get(request_url), share_code)
        .send()
        .await
        .map_err(|error| ShareListError::RequestFailed(error.to_string()))?;

    let body = response
        .json::<ListShareDirBody>()
        .await
        .map_err(|error| ShareListError::ParseFailed(error.to_string()))?;

    if body.res_code.is_text(RES_CODE_SHARE_NOT_FOUND) {
        return Err(ShareListError::InvalidShareCode);
    }

    if body.res_code.is_text(RES_CODE_SHARE_AUDIT_NOT_PASS) {
        return Err(ShareListError::Api("share audit not pass".to_string()));
    }

    if !body.res_code.is_success() {
        return Err(ShareListError::Api(body.res_message));
    }

    Ok(body)
}

pub(crate) fn with_share_headers(
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

fn deserialize_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;

    match value {
        serde_json::Value::String(text) => Ok(text),
        serde_json::Value::Number(number) => Ok(number.to_string()),
        serde_json::Value::Null => Ok(String::new()),
        _ => Err(serde::de::Error::custom("unexpected id value")),
    }
}