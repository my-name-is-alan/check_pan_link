use reqwest::Client;
use serde::Deserialize;
use std::{future::Future, pin::Pin};
use url::Url;

use crate::{
    checker::{
        Pan123ListType, Pan123ShareFile, Pan123ShareFolderNode, Pan123ShareListPayload,
        Pan123ShareListResponse, Pan123ShareNode, Provider,
    },
    error::ShareListError,
    providers::common::CheckContext,
};

const SHARE_INFO_ENDPOINT: &str = "https://www.123pan.com/gsb/s";
const SHARE_GET_ENDPOINT: &str = "https://www.123pan.com/b/api/share/get";
const SHARE_LIST_PAGE_LIMIT: usize = 100;
const ERRNO_INVALID_RECEIVE_CODE: i64 = 5_103;
const ERRNO_SHARE_LINK_INACTIVE: i64 = 5_104;

pub async fn list_share(
    context: CheckContext,
    list_type: Pan123ListType,
) -> Result<Pan123ShareListResponse, ShareListError> {
    list_share_with_endpoints(context, list_type, SHARE_INFO_ENDPOINT, SHARE_GET_ENDPOINT).await
}

pub(crate) async fn list_share_with_endpoints(
    context: CheckContext,
    list_type: Pan123ListType,
    share_info_endpoint: &str,
    share_get_endpoint: &str,
) -> Result<Pan123ShareListResponse, ShareListError> {
    let share_key = extract_share_key(&context.url).ok_or(ShareListError::InvalidPan123ShareUrl)?;
    let receive_code = extract_receive_code(&context.url);
    let share_info = fetch_share_info(
        &context.client,
        share_info_endpoint,
        &share_key,
        receive_code.as_deref(),
    )
    .await?;
    let root_listing = fetch_folder_listing(
        &context.client,
        share_get_endpoint,
        &share_key,
        receive_code.as_deref(),
        "0",
    )
    .await?;

    let share_name = share_info
        .share_name
        .clone()
        .filter(|name| !name.is_empty())
        .or_else(|| Some(share_key.clone()));
    let root_name = share_name.clone().unwrap_or_else(|| share_key.clone());
    let (root_file_id, root_parent_file_id, root_entries) = collapse_share_root_if_needed(
        &context.client,
        share_get_endpoint,
        &share_key,
        receive_code.as_deref(),
        &root_name,
        root_listing.entries,
    )
    .await?;
    let root_path = root_name.clone();

    let payload = match list_type {
        Pan123ListType::Files => {
            let mut files = collect_flat_files(
                &context.client,
                share_get_endpoint,
                &share_key,
                receive_code.as_deref(),
                root_entries,
                root_path,
            )
            .await?;
            files.sort_by(|left, right| left.path.cmp(&right.path));
            Pan123ShareListPayload::Files { files }
        }
        Pan123ListType::Tree => {
            let children = build_tree_children(
                &context.client,
                share_get_endpoint,
                &share_key,
                receive_code.as_deref(),
                root_entries,
                root_path.clone(),
            )
            .await?;
            Pan123ShareListPayload::Tree {
                tree: Pan123ShareFolderNode {
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
        Pan123ShareListPayload::Files { files } => files.len(),
        Pan123ShareListPayload::Tree { tree } => count_tree_files(&tree.children),
    };

    Ok(Pan123ShareListResponse {
        original_url: context.original_url,
        normalized_url: context.url.to_string(),
        provider: Provider::Pan123,
        list_type,
        share_key,
        receive_code,
        share_name,
        share_user_id: share_info.user_id,
        expired: share_info.expired,
        file_count,
        payload,
    })
}

async fn collapse_share_root_if_needed(
    client: &Client,
    endpoint: &str,
    share_key: &str,
    receive_code: Option<&str>,
    share_name: &str,
    entries: Vec<ShareEntry>,
) -> Result<(String, String, Vec<ShareEntry>), ShareListError> {
    if let [entry] = entries.as_slice() {
        if entry.is_folder() && entry.name == share_name {
            let listing =
                fetch_folder_listing(client, endpoint, share_key, receive_code, &entry.file_id)
                    .await?;
            return Ok((
                entry.file_id.clone(),
                entry.parent_file_id.clone(),
                listing.entries,
            ));
        }
    }

    Ok(("0".to_string(), "0".to_string(), entries))
}

async fn collect_flat_files(
    client: &Client,
    endpoint: &str,
    share_key: &str,
    receive_code: Option<&str>,
    root_entries: Vec<ShareEntry>,
    root_path: String,
) -> Result<Vec<Pan123ShareFile>, ShareListError> {
    let mut files = Vec::new();
    let mut stack = Vec::new();

    visit_entries_for_flat(root_entries, &root_path, &mut files, &mut stack);

    while let Some((file_id, path)) = stack.pop() {
        let listing =
            fetch_folder_listing(client, endpoint, share_key, receive_code, &file_id).await?;
        visit_entries_for_flat(listing.entries, &path, &mut files, &mut stack);
    }

    Ok(files)
}

fn visit_entries_for_flat(
    entries: Vec<ShareEntry>,
    base_path: &str,
    files: &mut Vec<Pan123ShareFile>,
    stack: &mut Vec<(String, String)>,
) {
    for entry in entries {
        let path = join_path(base_path, &entry.name);

        if entry.is_folder() {
            stack.push((entry.file_id, path));
            continue;
        }

        files.push(Pan123ShareFile {
            file_id: entry.file_id,
            parent_file_id: entry.parent_file_id,
            name: entry.name,
            path,
            size: entry.size,
            category: entry.category,
            status: entry.status,
        });
    }
}

fn build_tree_children<'a>(
    client: &'a Client,
    endpoint: &'a str,
    share_key: &'a str,
    receive_code: Option<&'a str>,
    entries: Vec<ShareEntry>,
    base_path: String,
) -> Pin<Box<dyn Future<Output = Result<Vec<Pan123ShareNode>, ShareListError>> + Send + 'a>> {
    Box::pin(async move {
        let mut children = Vec::new();

        for entry in entries {
            let path = join_path(&base_path, &entry.name);

            if entry.is_folder() {
                let listing =
                    fetch_folder_listing(client, endpoint, share_key, receive_code, &entry.file_id)
                        .await?;
                let nested_children = build_tree_children(
                    client,
                    endpoint,
                    share_key,
                    receive_code,
                    listing.entries,
                    path.clone(),
                )
                .await?;

                children.push(Pan123ShareNode::Folder {
                    file_id: entry.file_id,
                    parent_file_id: entry.parent_file_id,
                    name: entry.name,
                    path,
                    children: nested_children,
                });
                continue;
            }

            children.push(Pan123ShareNode::File {
                file_id: entry.file_id,
                parent_file_id: entry.parent_file_id,
                name: entry.name,
                path,
                size: entry.size,
                category: entry.category,
                status: entry.status,
            });
        }

        Ok(children)
    })
}

fn count_tree_files(nodes: &[Pan123ShareNode]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Pan123ShareNode::File { .. } => 1,
            Pan123ShareNode::Folder { children, .. } => count_tree_files(children),
        })
        .sum()
}

async fn fetch_share_info(
    client: &Client,
    endpoint: &str,
    share_key: &str,
    receive_code: Option<&str>,
) -> Result<ShareInfo, ShareListError> {
    let endpoint = format!("{}/{}", endpoint.trim_end_matches('/'), share_key);
    let request_url = Url::parse(&endpoint).map_err(|_| {
        ShareListError::RequestFailed("invalid 123 share info endpoint".to_string())
    })?;
    let response = client
        .get(request_url)
        .send()
        .await
        .map_err(map_request_error)?;
    let response = response
        .json::<ShareInfoResponse>()
        .await
        .map_err(|error| ShareListError::ParseFailed(error.to_string()))?;

    classify_share_info_response(response, receive_code.is_some())
}

async fn fetch_folder_listing(
    client: &Client,
    endpoint: &str,
    share_key: &str,
    receive_code: Option<&str>,
    parent_file_id: &str,
) -> Result<FolderListing, ShareListError> {
    let mut next = "0".to_string();
    let mut entries = Vec::new();

    loop {
        let response = fetch_share_get_page(
            client,
            endpoint,
            share_key,
            receive_code,
            parent_file_id,
            &next,
        )
        .await?;
        let data = response.data.ok_or_else(|| {
            ShareListError::Api("123 share list response is missing data".to_string())
        })?;

        let page_next = data.next.unwrap_or_default();
        let page_len = data.info_list.len();
        entries.extend(data.info_list.into_iter().map(ShareEntry::from));

        if page_len == 0 || page_next.is_empty() || page_next == "-1" {
            break;
        }

        next = page_next;
    }

    Ok(FolderListing { entries })
}

async fn fetch_share_get_page(
    client: &Client,
    endpoint: &str,
    share_key: &str,
    receive_code: Option<&str>,
    parent_file_id: &str,
    next: &str,
) -> Result<ShareGetResponse, ShareListError> {
    let limit = SHARE_LIST_PAGE_LIMIT.to_string();
    let mut query_params = vec![
        ("limit", limit.as_str()),
        ("next", next),
        ("orderBy", "file_name"),
        ("orderDirection", "asc"),
        ("shareKey", share_key),
        ("ParentFileId", parent_file_id),
        ("Page", "1"),
    ];

    if let Some(receive_code) = receive_code {
        query_params.push(("SharePwd", receive_code));
    }

    let request_url = Url::parse_with_params(endpoint, &query_params).map_err(|_| {
        ShareListError::RequestFailed("invalid 123 share list endpoint".to_string())
    })?;

    let response = client
        .get(request_url)
        .send()
        .await
        .map_err(map_request_error)?;

    response
        .json::<ShareGetResponse>()
        .await
        .map_err(|error| ShareListError::ParseFailed(error.to_string()))
        .and_then(classify_share_get_response)
}

fn map_request_error(error: reqwest::Error) -> ShareListError {
    if error.is_timeout() {
        ShareListError::RequestFailed("123 share list request timed out".to_string())
    } else if error.is_connect() {
        ShareListError::RequestFailed("123 share list connection failed".to_string())
    } else {
        ShareListError::RequestFailed(error.to_string())
    }
}

fn classify_share_info_response(
    response: ShareInfoResponse,
    receive_code_provided: bool,
) -> Result<ShareInfo, ShareListError> {
    if response.info.code == ERRNO_SHARE_LINK_INACTIVE {
        return Err(ShareListError::InvalidShareCode);
    }

    if response.info.code != 0 {
        return Err(ShareListError::Api(api_message(
            response.info.code,
            &response.info.message,
        )));
    }

    let data = response.info.data.ok_or_else(|| {
        ShareListError::Api("123 share info response is missing data".to_string())
    })?;

    if data.expired {
        return Err(ShareListError::InvalidShareCode);
    }

    if data.has_pwd && !receive_code_provided {
        return Err(ShareListError::MissingReceiveCode);
    }

    Ok(ShareInfo {
        share_name: Some(data.share_name),
        user_id: data.user_id,
        expired: data.expired,
    })
}

fn classify_share_get_response(
    response: ShareGetResponse,
) -> Result<ShareGetResponse, ShareListError> {
    match response.code {
        0 => {
            if response.data.as_ref().is_some_and(|data| data.expired) {
                Err(ShareListError::InvalidShareCode)
            } else {
                Ok(response)
            }
        }
        ERRNO_INVALID_RECEIVE_CODE => Err(ShareListError::InvalidReceiveCode),
        ERRNO_SHARE_LINK_INACTIVE => Err(ShareListError::InvalidShareCode),
        _ => Err(ShareListError::Api(api_message(
            response.code,
            &response.message,
        ))),
    }
}

fn api_message(code: i64, message: &str) -> String {
    if message.is_empty() {
        format!("code={code}")
    } else {
        format!("code={code}: {message}")
    }
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

fn join_path(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
}

struct ShareInfo {
    share_name: Option<String>,
    user_id: Option<u64>,
    expired: bool,
}

struct FolderListing {
    entries: Vec<ShareEntry>,
}

#[derive(Debug, Deserialize)]
struct ShareInfoResponse {
    info: ShareInfoPayload,
}

#[derive(Debug, Deserialize)]
struct ShareInfoPayload {
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: Option<ShareInfoData>,
}

#[derive(Debug, Deserialize)]
struct ShareInfoData {
    #[serde(rename = "UserID")]
    user_id: Option<u64>,
    #[serde(rename = "ShareName", default)]
    share_name: String,
    #[serde(rename = "HasPwd", default)]
    has_pwd: bool,
    #[serde(rename = "Expired", default)]
    expired: bool,
}

#[derive(Debug, Deserialize)]
struct ShareGetResponse {
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: Option<ShareGetData>,
}

#[derive(Debug, Deserialize)]
struct ShareGetData {
    #[serde(rename = "Next", default)]
    next: Option<String>,
    #[serde(rename = "Expired", default)]
    expired: bool,
    #[serde(rename = "InfoList", default)]
    info_list: Vec<ShareGetEntry>,
}

#[derive(Debug, Deserialize)]
struct ShareGetEntry {
    #[serde(rename = "FileId")]
    file_id: ShareIdentifier,
    #[serde(rename = "ParentFileId", default)]
    parent_file_id: Option<ShareIdentifier>,
    #[serde(rename = "FileName", default)]
    file_name: String,
    #[serde(rename = "Type", default)]
    file_type: i64,
    #[serde(rename = "Size", default)]
    size: u64,
    #[serde(rename = "Etag", default)]
    etag: Option<String>,
    #[serde(rename = "Category", default)]
    category: Option<i64>,
    #[serde(rename = "Status", default)]
    status: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ShareIdentifier {
    Text(String),
    Unsigned(u64),
    Signed(i64),
}

impl ShareIdentifier {
    fn into_string(self) -> String {
        match self {
            Self::Text(value) => value,
            Self::Unsigned(value) => value.to_string(),
            Self::Signed(value) => value.to_string(),
        }
    }
}

struct ShareEntry {
    file_id: String,
    parent_file_id: String,
    name: String,
    size: u64,
    category: Option<i64>,
    status: Option<i64>,
    file_type: i64,
}

impl ShareEntry {
    fn is_folder(&self) -> bool {
        self.file_type == 1
    }
}

impl From<ShareGetEntry> for ShareEntry {
    fn from(value: ShareGetEntry) -> Self {
        let file_id = value.file_id.into_string();
        Self {
            file_id,
            parent_file_id: value
                .parent_file_id
                .map(ShareIdentifier::into_string)
                .unwrap_or_else(|| "0".to_string()),
            name: value.file_name,
            size: value.size,
            category: value.category,
            status: value.status,
            file_type: value.file_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        extract::{Path, Query, State},
        routing::get,
    };
    use serde_json::{Value, json};
    use std::{collections::BTreeMap, sync::Arc};
    use tokio::net::TcpListener;

    use crate::{
        checker::{
            Pan123ListType, Pan123ShareListPayload, Pan123ShareListRequest, Provider,
            service::LinkCheckerService,
        },
        error::ShareListError,
    };

    #[tokio::test]
    async fn lists_nested_share_files_in_files_mode() {
        let (share_info_endpoint, share_get_endpoint) =
            spawn_share_api(mock_share_info(true), mock_nested_share_responses()).await;
        let service = build_service();

        let response = service
            .list_pan123_share_with_endpoints(
                Pan123ShareListRequest {
                    url: "https://www.123865.com/s/IpPUVv-gGKj?pwd=Ocat".to_string(),
                    list_type: Pan123ListType::Files,
                },
                &share_info_endpoint,
                &share_get_endpoint,
            )
            .await
            .unwrap();

        assert_eq!(response.provider, Provider::Pan123);
        assert_eq!(response.file_count, 3);
        assert_eq!(response.share_name.as_deref(), Some("鲭鱼罐头，飞向宇宙"));

        let Pan123ShareListPayload::Files { files } = response.payload else {
            panic!("expected flat file payload");
        };

        let paths = files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                "鲭鱼罐头，飞向宇宙/Season 1/Episode 01.mkv",
                "鲭鱼罐头，飞向宇宙/Season 1/Episode 02.mkv",
                "鲭鱼罐头，飞向宇宙/Season 2/Episode 01.mkv",
            ]
        );
    }

    #[tokio::test]
    async fn lists_nested_share_files_in_tree_mode() {
        let (share_info_endpoint, share_get_endpoint) =
            spawn_share_api(mock_share_info(true), mock_nested_share_responses()).await;
        let service = build_service();

        let response = service
            .list_pan123_share_with_endpoints(
                Pan123ShareListRequest {
                    url: "https://www.123865.com/s/IpPUVv-gGKj?pwd=Ocat".to_string(),
                    list_type: Pan123ListType::Tree,
                },
                &share_info_endpoint,
                &share_get_endpoint,
            )
            .await
            .unwrap();

        let Pan123ShareListPayload::Tree { tree } = response.payload else {
            panic!("expected tree payload");
        };

        assert_eq!(tree.name, "鲭鱼罐头，飞向宇宙");
        assert_eq!(tree.children.len(), 2);
    }

    #[tokio::test]
    async fn returns_missing_receive_code_error_when_share_requires_password() {
        let (share_info_endpoint, share_get_endpoint) =
            spawn_share_api(mock_share_info(true), BTreeMap::new()).await;
        let service = build_service();

        let error = service
            .list_pan123_share_with_endpoints(
                Pan123ShareListRequest {
                    url: "https://www.123865.com/s/IpPUVv-gGKj".to_string(),
                    list_type: Pan123ListType::Files,
                },
                &share_info_endpoint,
                &share_get_endpoint,
            )
            .await
            .unwrap_err();

        assert!(matches!(error, ShareListError::MissingReceiveCode));
    }

    #[tokio::test]
    async fn returns_invalid_receive_code_error() {
        let (share_info_endpoint, share_get_endpoint) = spawn_share_api(
            mock_share_info(true),
            BTreeMap::from([(
                key("0", "0"),
                json!({"code": 5103, "message": "提取码错误", "data": null}),
            )]),
        )
        .await;
        let service = build_service();

        let error = service
            .list_pan123_share_with_endpoints(
                Pan123ShareListRequest {
                    url: "https://www.123865.com/s/IpPUVv-gGKj?pwd=bad".to_string(),
                    list_type: Pan123ListType::Files,
                },
                &share_info_endpoint,
                &share_get_endpoint,
            )
            .await
            .unwrap_err();

        assert!(matches!(error, ShareListError::InvalidReceiveCode));
    }

    fn build_service() -> LinkCheckerService {
        LinkCheckerService::new(std::time::Duration::from_secs(1)).unwrap()
    }

    fn mock_share_info(has_pwd: bool) -> Value {
        json!({
            "info": {
                "code": 0,
                "message": "",
                "data": {
                    "UserID": 1813308069,
                    "ShareName": "鲭鱼罐头，飞向宇宙",
                    "HasPwd": has_pwd,
                    "Expired": false,
                    "ShareKey": "IpPUVv-gGKj"
                }
            }
        })
    }

    fn mock_nested_share_responses() -> BTreeMap<String, Value> {
        BTreeMap::from([
            (
                key("0", "0"),
                json!({
                    "code": 0,
                    "message": "ok",
                    "data": {
                        "Next": "-1",
                        "Expired": false,
                        "InfoList": [
                            {
                                "FileId": "folder-root",
                                "ParentFileId": 0,
                                "FileName": "鲭鱼罐头，飞向宇宙",
                                "Type": 1,
                                "Size": 300,
                                "Category": 0,
                                "Status": 0
                            }
                        ]
                    }
                }),
            ),
            (
                key("folder-root", "0"),
                json!({
                    "code": 0,
                    "message": "ok",
                    "data": {
                        "Next": "-1",
                        "Expired": false,
                        "InfoList": [
                            {
                                "FileId": "season-1",
                                "ParentFileId": "folder-root",
                                "FileName": "Season 1",
                                "Type": 1,
                                "Size": 200,
                                "Category": 0,
                                "Status": 0
                            },
                            {
                                "FileId": "season-2",
                                "ParentFileId": "folder-root",
                                "FileName": "Season 2",
                                "Type": 1,
                                "Size": 100,
                                "Category": 0,
                                "Status": 0
                            }
                        ]
                    }
                }),
            ),
            (
                key("season-1", "0"),
                json!({
                    "code": 0,
                    "message": "ok",
                    "data": {
                        "Next": "-1",
                        "Expired": false,
                        "InfoList": [
                            {
                                "FileId": "file-1",
                                "ParentFileId": "season-1",
                                "FileName": "Episode 01.mkv",
                                "Type": 0,
                                "Size": 100,
                                "Etag": "etag-1",
                                "Category": 2,
                                "Status": 2
                            },
                            {
                                "FileId": "file-2",
                                "ParentFileId": "season-1",
                                "FileName": "Episode 02.mkv",
                                "Type": 0,
                                "Size": 200,
                                "Etag": "etag-2",
                                "Category": 2,
                                "Status": 2
                            }
                        ]
                    }
                }),
            ),
            (
                key("season-2", "0"),
                json!({
                    "code": 0,
                    "message": "ok",
                    "data": {
                        "Next": "-1",
                        "Expired": false,
                        "InfoList": [
                            {
                                "FileId": "file-3",
                                "ParentFileId": "season-2",
                                "FileName": "Episode 01.mkv",
                                "Type": 0,
                                "Size": 300,
                                "Etag": "etag-3",
                                "Category": 2,
                                "Status": 2
                            }
                        ]
                    }
                }),
            ),
        ])
    }

    fn key(parent_file_id: &str, next: &str) -> String {
        format!("{parent_file_id}:{next}")
    }

    async fn spawn_share_api(
        share_info_response: Value,
        share_get_responses: BTreeMap<String, Value>,
    ) -> (String, String) {
        #[derive(Clone)]
        struct MockState {
            share_info_response: Value,
            share_get_responses: Arc<BTreeMap<String, Value>>,
        }

        async fn share_info(
            State(state): State<MockState>,
            Path(_share_key): Path<String>,
        ) -> Json<Value> {
            Json(state.share_info_response)
        }

        async fn share_get(
            State(state): State<MockState>,
            Query(query): Query<BTreeMap<String, String>>,
        ) -> Json<Value> {
            let parent_file_id = query
                .get("ParentFileId")
                .cloned()
                .unwrap_or_else(|| "0".to_string());
            let next = query
                .get("next")
                .cloned()
                .unwrap_or_else(|| "0".to_string());
            let key = format!("{parent_file_id}:{next}");

            Json(state.share_get_responses.get(&key).cloned().unwrap_or_else(
                || json!({"code": 999999, "message": "missing mock response", "data": null}),
            ))
        }

        let app = Router::new()
            .route("/gsb/s/{share_key}", get(share_info))
            .route("/b/api/share/get", get(share_get))
            .with_state(MockState {
                share_info_response,
                share_get_responses: Arc::new(share_get_responses),
            });

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        (
            format!("http://{address}/gsb/s"),
            format!("http://{address}/b/api/share/get"),
        )
    }
}
