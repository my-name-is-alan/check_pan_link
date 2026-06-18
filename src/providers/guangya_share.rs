use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{future::Future, pin::Pin};

use crate::{
    checker::{
        GuangyaListType, GuangyaShareFile, GuangyaShareFolderNode, GuangyaShareListPayload,
        GuangyaShareListResponse, GuangyaShareNode, Provider,
    },
    error::ShareListError,
    providers::{
        common::CheckContext,
        guangya::{
            ACCESS_TOKEN_ENDPOINT, CODE_INVALID_RECEIVE_CODE, CODE_INVALID_SHARE,
            CODE_INVALID_SHARE_ALT, CODE_INVALID_SHARE_ID, CODE_SHARE_EXPIRED, GuangyaApiResponse,
            SHARE_STATUS_EXPIRED, SHARE_SUMMARY_ENDPOINT, ShareSummaryData, extract_receive_code,
            extract_share_id, fetch_access_token, fetch_share_summary, inactive_share_reason,
            is_share_status_available, post_guangya_json, share_status_label,
        },
    },
};

const SHARE_FILE_LIST_ENDPOINT: &str =
    "https://api.guangyapan.com/userres/v1/get_share_page_files_list";
const SHARE_LIST_PAGE_LIMIT: usize = 100;

pub async fn list_share(
    context: CheckContext,
    list_type: GuangyaListType,
) -> Result<GuangyaShareListResponse, ShareListError> {
    list_share_with_endpoints(
        context,
        list_type,
        SHARE_SUMMARY_ENDPOINT,
        ACCESS_TOKEN_ENDPOINT,
        SHARE_FILE_LIST_ENDPOINT,
    )
    .await
}

pub(crate) async fn list_share_with_endpoints(
    context: CheckContext,
    list_type: GuangyaListType,
    summary_endpoint: &str,
    token_endpoint: &str,
    list_endpoint: &str,
) -> Result<GuangyaShareListResponse, ShareListError> {
    let share_id = extract_share_id(&context.url).ok_or(ShareListError::InvalidGuangyaShareUrl)?;
    let receive_code = extract_receive_code(&context.url);
    let summary = fetch_share_summary(&context.client, summary_endpoint, &share_id)
        .await
        .map_err(map_guangya_request_error)?;
    let summary_data = classify_summary(summary, receive_code.is_some())?;
    let token = fetch_access_token(
        &context.client,
        token_endpoint,
        &share_id,
        receive_code.as_deref(),
    )
    .await
    .map_err(map_guangya_request_error)?;
    let access_token = classify_access_token(token)?;

    let share_title = summary_data
        .title
        .clone()
        .filter(|title| !title.is_empty())
        .or_else(|| Some(share_id.clone()));
    let root_name = share_title.clone().unwrap_or_else(|| share_id.clone());
    let root_listing =
        fetch_folder_listing(&context.client, list_endpoint, &access_token, "").await?;
    let (root_file_id, root_parent_file_id, root_entries) = collapse_share_root_if_needed(
        &context.client,
        list_endpoint,
        &access_token,
        &root_name,
        root_listing.entries,
    )
    .await?;
    let root_path = root_name.clone();

    let payload = match list_type {
        GuangyaListType::Files => {
            let mut files = collect_flat_files(
                &context.client,
                list_endpoint,
                &access_token,
                root_entries,
                root_path,
            )
            .await?;
            files.sort_by(|left, right| left.path.cmp(&right.path));
            GuangyaShareListPayload::Files { files }
        }
        GuangyaListType::Tree => {
            let children = build_tree_children(
                &context.client,
                list_endpoint,
                &access_token,
                root_entries,
                root_path.clone(),
            )
            .await?;
            GuangyaShareListPayload::Tree {
                tree: GuangyaShareFolderNode {
                    file_id: root_file_id,
                    parent_file_id: root_parent_file_id,
                    name: root_name,
                    path: root_path,
                    children,
                },
            }
        }
    };

    let file_count = match &payload {
        GuangyaShareListPayload::Files { files } => files.len(),
        GuangyaShareListPayload::Tree { tree } => count_tree_files(&tree.children),
    };

    Ok(GuangyaShareListResponse {
        original_url: context.original_url,
        normalized_url: context.url.to_string(),
        provider: Provider::GuangyaPan,
        list_type,
        share_id,
        receive_code,
        share_title,
        share_status: summary_data.share_status,
        share_status_label: summary_data
            .share_status
            .map(|status| share_status_label(status).to_string()),
        share_available: is_share_status_available(summary_data.share_status),
        share_user_id: summary_data.user_id,
        need_receive_code: summary_data.need_code,
        file_count,
        payload,
    })
}

async fn collapse_share_root_if_needed(
    client: &Client,
    endpoint: &str,
    access_token: &str,
    share_name: &str,
    entries: Vec<ShareEntry>,
) -> Result<(String, String, Vec<ShareEntry>), ShareListError> {
    if let [entry] = entries.as_slice() {
        if entry.is_folder() && entry.name == share_name {
            let listing =
                fetch_folder_listing(client, endpoint, access_token, &entry.file_id).await?;
            return Ok((
                entry.file_id.clone(),
                entry.parent_file_id.clone(),
                listing.entries,
            ));
        }
    }

    Ok(("".to_string(), "".to_string(), entries))
}

async fn collect_flat_files(
    client: &Client,
    endpoint: &str,
    access_token: &str,
    root_entries: Vec<ShareEntry>,
    root_path: String,
) -> Result<Vec<GuangyaShareFile>, ShareListError> {
    let mut files = Vec::new();
    let mut stack = Vec::new();

    visit_entries_for_flat(root_entries, &root_path, &mut files, &mut stack);

    while let Some((file_id, path)) = stack.pop() {
        let listing = fetch_folder_listing(client, endpoint, access_token, &file_id).await?;
        visit_entries_for_flat(listing.entries, &path, &mut files, &mut stack);
    }

    Ok(files)
}

fn visit_entries_for_flat(
    entries: Vec<ShareEntry>,
    base_path: &str,
    files: &mut Vec<GuangyaShareFile>,
    stack: &mut Vec<(String, String)>,
) {
    for entry in entries {
        let path = join_path(base_path, &entry.name);

        if entry.is_folder() {
            stack.push((entry.file_id, path));
            continue;
        }

        files.push(GuangyaShareFile {
            file_id: entry.file_id,
            parent_file_id: entry.parent_file_id,
            name: entry.name,
            path,
            size: entry.size,
            extension: entry.extension,
            file_type: entry.file_type,
            audit_status: entry.audit_status,
            audit_status_label: audit_status_label(entry.audit_status).map(str::to_string),
            is_available: is_file_available(entry.audit_status),
        });
    }
}

fn build_tree_children<'a>(
    client: &'a Client,
    endpoint: &'a str,
    access_token: &'a str,
    entries: Vec<ShareEntry>,
    base_path: String,
) -> Pin<Box<dyn Future<Output = Result<Vec<GuangyaShareNode>, ShareListError>> + Send + 'a>> {
    Box::pin(async move {
        let mut children = Vec::new();

        for entry in entries {
            let path = join_path(&base_path, &entry.name);

            if entry.is_folder() {
                let listing =
                    fetch_folder_listing(client, endpoint, access_token, &entry.file_id).await?;
                let nested_children = build_tree_children(
                    client,
                    endpoint,
                    access_token,
                    listing.entries,
                    path.clone(),
                )
                .await?;

                children.push(GuangyaShareNode::Folder {
                    file_id: entry.file_id,
                    parent_file_id: entry.parent_file_id,
                    name: entry.name,
                    path,
                    children: nested_children,
                });
                continue;
            }

            children.push(GuangyaShareNode::File {
                file_id: entry.file_id,
                parent_file_id: entry.parent_file_id,
                name: entry.name,
                path,
                size: entry.size,
                extension: entry.extension,
                file_type: entry.file_type,
                audit_status: entry.audit_status,
                audit_status_label: audit_status_label(entry.audit_status).map(str::to_string),
                is_available: is_file_available(entry.audit_status),
            });
        }

        Ok(children)
    })
}

fn count_tree_files(nodes: &[GuangyaShareNode]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            GuangyaShareNode::File { .. } => 1,
            GuangyaShareNode::Folder { children, .. } => count_tree_files(children),
        })
        .sum()
}

async fn fetch_folder_listing(
    client: &Client,
    endpoint: &str,
    access_token: &str,
    parent_id: &str,
) -> Result<FolderListing, ShareListError> {
    let mut cursor = None;
    let mut entries = Vec::new();

    loop {
        let response =
            fetch_list_page(client, endpoint, access_token, parent_id, cursor.as_deref()).await?;
        let data = response.data.ok_or_else(|| {
            ShareListError::Api("Guangya share list response is missing data".to_string())
        })?;

        entries.extend(data.list.into_iter().map(ShareEntry::from));

        if !data.has_more.unwrap_or(false) {
            break;
        }

        let Some(next_cursor) = data.cursor.as_ref().and_then(value_to_cursor) else {
            break;
        };
        cursor = Some(next_cursor);
    }

    Ok(FolderListing { entries })
}

async fn fetch_list_page(
    client: &Client,
    endpoint: &str,
    access_token: &str,
    parent_id: &str,
    cursor: Option<&str>,
) -> Result<GuangyaApiResponse<FileListData>, ShareListError> {
    let response = post_guangya_json(
        client,
        endpoint,
        &FileListRequest {
            access_token,
            page_size: SHARE_LIST_PAGE_LIMIT,
            order_by: 0,
            sort_type: 0,
            parent_id,
            cursor,
        },
    )
    .await
    .map_err(map_guangya_request_error)?;

    classify_list_response(response)
}

fn classify_summary(
    response: GuangyaApiResponse<ShareSummaryData>,
    receive_code_provided: bool,
) -> Result<ShareSummaryData, ShareListError> {
    match response.code() {
        0 => {
            let data = response.data.ok_or_else(|| {
                ShareListError::Api("Guangya share summary response is missing data".to_string())
            })?;
            if data.need_code && !receive_code_provided {
                Err(ShareListError::MissingReceiveCode)
            } else if let Some(reason) = inactive_share_reason(data.share_status) {
                Err(share_status_error(reason, data.share_status))
            } else {
                Ok(data)
            }
        }
        CODE_INVALID_RECEIVE_CODE => Err(ShareListError::InvalidReceiveCode),
        CODE_INVALID_SHARE_ID | CODE_INVALID_SHARE | CODE_INVALID_SHARE_ALT => {
            Err(ShareListError::InvalidShareCode)
        }
        CODE_SHARE_EXPIRED => Err(ShareListError::ShareExpired),
        code => Err(ShareListError::Api(api_message(code, &response.msg))),
    }
}

fn classify_access_token(
    response: GuangyaApiResponse<crate::providers::guangya::AccessTokenData>,
) -> Result<String, ShareListError> {
    match response.code() {
        0 => response
            .data
            .map(|data| data.access_token)
            .filter(|access_token| !access_token.is_empty())
            .ok_or_else(|| {
                ShareListError::Api("Guangya access token response is missing data".to_string())
            }),
        CODE_INVALID_RECEIVE_CODE => Err(ShareListError::InvalidReceiveCode),
        CODE_INVALID_SHARE_ID | CODE_INVALID_SHARE | CODE_INVALID_SHARE_ALT => {
            Err(ShareListError::InvalidShareCode)
        }
        CODE_SHARE_EXPIRED => Err(ShareListError::ShareExpired),
        code => Err(ShareListError::Api(api_message(code, &response.msg))),
    }
}

fn classify_list_response(
    response: GuangyaApiResponse<FileListData>,
) -> Result<GuangyaApiResponse<FileListData>, ShareListError> {
    match response.code() {
        0 => Ok(response),
        CODE_INVALID_RECEIVE_CODE => Err(ShareListError::InvalidReceiveCode),
        CODE_INVALID_SHARE_ID | CODE_INVALID_SHARE | CODE_INVALID_SHARE_ALT => {
            Err(ShareListError::InvalidShareCode)
        }
        CODE_SHARE_EXPIRED => Err(ShareListError::ShareExpired),
        code => Err(ShareListError::Api(api_message(code, &response.msg))),
    }
}

fn share_status_error(reason: &str, share_status: Option<i64>) -> ShareListError {
    if reason == "share_expired" || share_status == Some(SHARE_STATUS_EXPIRED) {
        ShareListError::ShareExpired
    } else {
        ShareListError::InvalidShareCode
    }
}

fn audit_status_label(audit_status: Option<i64>) -> Option<&'static str> {
    match audit_status {
        Some(3) => Some("violation"),
        Some(4) => Some("available"),
        Some(_) => Some("unknown"),
        None => None,
    }
}

fn is_file_available(audit_status: Option<i64>) -> bool {
    audit_status != Some(3)
}

fn map_guangya_request_error(
    error: crate::providers::guangya::GuangyaRequestError,
) -> ShareListError {
    match error {
        crate::providers::guangya::GuangyaRequestError::Request(error) => {
            ShareListError::RequestFailed(error.to_string())
        }
        crate::providers::guangya::GuangyaRequestError::Parse(error) => {
            ShareListError::ParseFailed(error.to_string())
        }
    }
}

fn api_message(code: i64, message: &str) -> String {
    if message.is_empty() {
        format!("code={code}")
    } else {
        format!("code={code}: {message}")
    }
}

fn value_to_cursor(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn join_path(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileListRequest<'a> {
    access_token: &'a str,
    page_size: usize,
    order_by: i64,
    sort_type: i64,
    parent_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cursor: Option<&'a str>,
}

struct FolderListing {
    entries: Vec<ShareEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileListData {
    #[serde(default)]
    list: Vec<FileListEntry>,
    #[serde(default)]
    cursor: Option<Value>,
    #[serde(default)]
    has_more: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileListEntry {
    file_id: String,
    #[serde(default)]
    parent_id: String,
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    file_size: u64,
    #[serde(default)]
    res_type: i64,
    #[serde(default)]
    ext: Option<String>,
    #[serde(default)]
    file_type: Option<i64>,
    #[serde(default)]
    audit_status: Option<i64>,
}

struct ShareEntry {
    file_id: String,
    parent_file_id: String,
    name: String,
    size: u64,
    resource_type: i64,
    extension: Option<String>,
    file_type: Option<i64>,
    audit_status: Option<i64>,
}

impl ShareEntry {
    fn is_folder(&self) -> bool {
        self.resource_type == 2
    }
}

impl From<FileListEntry> for ShareEntry {
    fn from(value: FileListEntry) -> Self {
        Self {
            file_id: value.file_id,
            parent_file_id: value.parent_id,
            name: value.file_name,
            size: value.file_size,
            resource_type: value.res_type,
            extension: value.ext,
            file_type: value.file_type,
            audit_status: value.audit_status,
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{Json, Router, extract::State, routing::post};
    use serde_json::{Value, json};
    use std::{collections::BTreeMap, sync::Arc};
    use tokio::net::TcpListener;

    use super::*;
    use crate::{
        checker::{GuangyaListType, GuangyaShareListPayload, Provider},
        providers::common::CheckContext,
    };

    #[tokio::test]
    async fn lists_nested_guangya_share_files_in_files_mode() {
        let (summary_endpoint, token_endpoint, list_endpoint) = spawn_share_api(
            mock_summary(true),
            mock_token(),
            mock_nested_list_responses(),
        )
        .await;
        let context = CheckContext {
            original_url:
                "https://www.guangyapan.com/s/1910961611262906426_adz3Lo8EdLN_2BBy?code=bmiu"
                    .to_string(),
            url: url::Url::parse(
                "https://www.guangyapan.com/s/1910961611262906426_adz3Lo8EdLN_2BBy?code=bmiu",
            )
            .unwrap(),
            client: build_test_client(),
        };

        let response = list_share_with_endpoints(
            context,
            GuangyaListType::Files,
            &summary_endpoint,
            &token_endpoint,
            &list_endpoint,
        )
        .await
        .unwrap();

        assert_eq!(response.provider, Provider::GuangyaPan);
        assert_eq!(response.file_count, 2);
        assert_eq!(response.share_title.as_deref(), Some("瑞奇冲冲冲"));

        let GuangyaShareListPayload::Files { files } = response.payload else {
            panic!("expected files payload");
        };
        let paths = files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            paths,
            vec![
                "瑞奇冲冲冲/Season 1/Episode 01.mp4",
                "瑞奇冲冲冲/Season 1/Episode 02.mp4",
            ]
        );
    }

    #[tokio::test]
    async fn lists_nested_guangya_share_files_in_tree_mode() {
        let (summary_endpoint, token_endpoint, list_endpoint) = spawn_share_api(
            mock_summary(false),
            mock_token(),
            mock_nested_list_responses(),
        )
        .await;
        let context = CheckContext {
            original_url: "https://www.guangyapan.com/s/1910961250145955889_adz3Lo8EdLN_2BBy"
                .to_string(),
            url: url::Url::parse(
                "https://www.guangyapan.com/s/1910961250145955889_adz3Lo8EdLN_2BBy",
            )
            .unwrap(),
            client: build_test_client(),
        };

        let response = list_share_with_endpoints(
            context,
            GuangyaListType::Tree,
            &summary_endpoint,
            &token_endpoint,
            &list_endpoint,
        )
        .await
        .unwrap();

        let GuangyaShareListPayload::Tree { tree } = response.payload else {
            panic!("expected tree payload");
        };

        assert_eq!(tree.name, "瑞奇冲冲冲");
        assert_eq!(tree.children.len(), 1);
    }

    #[tokio::test]
    async fn rejects_expired_share_status_before_listing_files() {
        let mut summary = mock_summary(false);
        summary["data"]["shareStatus"] = json!(2);
        let (summary_endpoint, token_endpoint, list_endpoint) =
            spawn_share_api(summary, mock_token(), mock_nested_list_responses()).await;
        let context = CheckContext {
            original_url: "https://www.guangyapan.com/s/1910961250145955889_adz3Lo8EdLN_2BBy"
                .to_string(),
            url: url::Url::parse(
                "https://www.guangyapan.com/s/1910961250145955889_adz3Lo8EdLN_2BBy",
            )
            .unwrap(),
            client: build_test_client(),
        };

        let error = list_share_with_endpoints(
            context,
            GuangyaListType::Files,
            &summary_endpoint,
            &token_endpoint,
            &list_endpoint,
        )
        .await
        .unwrap_err();

        assert!(
            matches!(error, ShareListError::ShareExpired),
            "unexpected error: {error:?}"
        );
    }

    #[tokio::test]
    async fn labels_violation_files_as_unavailable() {
        let (summary_endpoint, token_endpoint, list_endpoint) = spawn_share_api(
            mock_summary(false),
            mock_token(),
            mock_violation_list_responses(),
        )
        .await;
        let context = CheckContext {
            original_url: "https://www.guangyapan.com/s/1910961250145955889_adz3Lo8EdLN_2BBy"
                .to_string(),
            url: url::Url::parse(
                "https://www.guangyapan.com/s/1910961250145955889_adz3Lo8EdLN_2BBy",
            )
            .unwrap(),
            client: build_test_client(),
        };

        let response = list_share_with_endpoints(
            context,
            GuangyaListType::Files,
            &summary_endpoint,
            &token_endpoint,
            &list_endpoint,
        )
        .await
        .unwrap();
        let value = serde_json::to_value(response).unwrap();

        assert_eq!(value["files"][0]["audit_status"], json!(3));
        assert_eq!(value["files"][0]["audit_status_label"], json!("violation"));
        assert_eq!(value["files"][0]["is_available"], json!(false));
    }

    fn mock_summary(need_code: bool) -> Value {
        json!({
            "msg": "success",
            "data": {
                "needCode": need_code,
                "shareStatus": 1,
                "title": "瑞奇冲冲冲",
                "userId": "adz3Lo8EdLN_2BBy",
                "leftTime": -1
            }
        })
    }

    fn mock_token() -> Value {
        json!({"msg": "success", "data": {"accessToken": "share-token"}})
    }

    fn mock_nested_list_responses() -> BTreeMap<String, Value> {
        BTreeMap::from([
            (
                "".to_string(),
                json!({
                    "msg": "success",
                    "data": {
                        "total": 1,
                        "list": [{
                            "fileId": "folder-root",
                            "fileName": "瑞奇冲冲冲",
                            "parentId": "root-parent",
                            "resType": 2,
                            "ctime": 1776762121,
                            "utime": 1776762121
                        }],
                        "cursor": 10
                    }
                }),
            ),
            (
                "folder-root".to_string(),
                json!({
                    "msg": "success",
                    "data": {
                        "total": 1,
                        "list": [{
                            "fileId": "season-1",
                            "fileName": "Season 1",
                            "parentId": "folder-root",
                            "resType": 2,
                            "ctime": 1776762121,
                            "utime": 1776762121
                        }],
                        "cursor": 10
                    }
                }),
            ),
            (
                "season-1".to_string(),
                json!({
                    "msg": "success",
                    "data": {
                        "total": 2,
                        "list": [
                            {
                                "fileId": "file-1",
                                "fileName": "Episode 01.mp4",
                                "fileSize": 100,
                                "parentId": "season-1",
                                "resType": 1,
                                "fileType": 2,
                                "ext": ".mp4",
                                "auditStatus": 4
                            },
                            {
                                "fileId": "file-2",
                                "fileName": "Episode 02.mp4",
                                "fileSize": 200,
                                "parentId": "season-1",
                                "resType": 1,
                                "fileType": 2,
                                "ext": ".mp4",
                                "auditStatus": 4
                            }
                        ],
                        "cursor": 10
                    }
                }),
            ),
        ])
    }

    fn mock_violation_list_responses() -> BTreeMap<String, Value> {
        BTreeMap::from([(
            "".to_string(),
            json!({
                "msg": "success",
                "data": {
                    "total": 1,
                    "list": [{
                        "fileId": "file-violation",
                        "fileName": "blocked.mp4",
                        "fileSize": 100,
                        "parentId": "",
                        "resType": 1,
                        "fileType": 2,
                        "ext": ".mp4",
                        "auditStatus": 3
                    }],
                    "cursor": 10
                }
            }),
        )])
    }

    fn build_test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .no_proxy()
            .build()
            .unwrap()
    }

    async fn spawn_share_api(
        summary_response: Value,
        token_response: Value,
        list_responses: BTreeMap<String, Value>,
    ) -> (String, String, String) {
        #[derive(Clone)]
        struct MockState {
            summary_response: Value,
            token_response: Value,
            list_responses: Arc<BTreeMap<String, Value>>,
        }

        async fn summary(State(state): State<MockState>) -> Json<Value> {
            Json(state.summary_response)
        }

        async fn token(State(state): State<MockState>) -> Json<Value> {
            Json(state.token_response)
        }

        async fn list(State(state): State<MockState>, Json(body): Json<Value>) -> Json<Value> {
            let parent_id = body
                .get("parentId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            Json(
                state
                    .list_responses
                    .get(&parent_id)
                    .cloned()
                    .unwrap_or_else(|| json!({"code": 143, "msg": "文件不存在"})),
            )
        }

        let app = Router::new()
            .route("/summary", post(summary))
            .route("/token", post(token))
            .route("/list", post(list))
            .with_state(MockState {
                summary_response,
                token_response,
                list_responses: Arc::new(list_responses),
            });

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        (
            format!("http://{address}/summary"),
            format!("http://{address}/token"),
            format!("http://{address}/list"),
        )
    }
}
