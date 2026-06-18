use reqwest::Client;
use serde::Deserialize;
use std::{future::Future, pin::Pin};
use url::Url;

use crate::{
    checker::{
        Pan115ListType, Pan115ShareFile, Pan115ShareFolderNode, Pan115ShareListPayload,
        Pan115ShareListResponse, Pan115ShareNode, Provider,
    },
    error::ShareListError,
    providers::{common::CheckContext, pan115_headers::with_share_snap_headers},
};

const SHARE_SNAP_ENDPOINT: &str = "https://webapi.115.com/share/snap";
const SHARE_LIST_PAGE_LIMIT: usize = 200;
const ERRNO_INVALID_RECEIVE_CODE: i64 = 4_100_008;
const ERRNO_MISSING_RECEIVE_CODE: i64 = 4_100_012;
const ERRNO_INVALID_SHARE_CODE: i64 = 990_002;

pub async fn list_share(
    context: CheckContext,
    list_type: Pan115ListType,
) -> Result<Pan115ShareListResponse, ShareListError> {
    list_share_with_endpoint(context, list_type, SHARE_SNAP_ENDPOINT).await
}

pub(crate) async fn list_share_with_endpoint(
    context: CheckContext,
    list_type: Pan115ListType,
    endpoint: &str,
) -> Result<Pan115ShareListResponse, ShareListError> {
    let share_code =
        extract_share_code(&context.url).ok_or(ShareListError::InvalidPan115ShareUrl)?;
    let receive_code = extract_receive_code(&context.url);
    let root_listing = fetch_folder_listing(
        &context.client,
        endpoint,
        &share_code,
        receive_code.as_deref(),
        "0",
    )
    .await?;

    let share_title = root_listing
        .share_title
        .clone()
        .filter(|title| !title.is_empty())
        .or_else(|| Some(share_code.clone()));
    let root_name = share_title.clone().unwrap_or_else(|| share_code.clone());
    let share_state = root_listing.share_state;
    let share_state_label = share_state.map(|state| share_state_label(state).to_string());

    let (root_cid, root_entries, root_path) = collapse_share_root_if_needed(
        &context.client,
        endpoint,
        &share_code,
        receive_code.as_deref(),
        &root_name,
        root_listing.entries,
    )
    .await?;

    let payload = match list_type {
        Pan115ListType::Files => {
            let mut files = collect_flat_files(
                &context.client,
                endpoint,
                &share_code,
                receive_code.as_deref(),
                root_entries,
                root_path.clone(),
            )
            .await?;
            files.sort_by(|left, right| left.path.cmp(&right.path));
            Pan115ShareListPayload::Files { files }
        }
        Pan115ListType::Tree => {
            let children = build_tree_children(
                &context.client,
                endpoint,
                &share_code,
                receive_code.as_deref(),
                root_entries,
                root_path.clone(),
            )
            .await?;
            Pan115ShareListPayload::Tree {
                tree: Pan115ShareFolderNode {
                    cid: root_cid,
                    name: root_name.clone(),
                    path: root_path,
                    children,
                },
            }
        }
    };

    let file_count = match &payload {
        Pan115ShareListPayload::Files { files } => files.len(),
        Pan115ShareListPayload::Tree { tree } => count_tree_files(&tree.children),
    };

    Ok(Pan115ShareListResponse {
        original_url: context.original_url,
        normalized_url: context.url.to_string(),
        provider: Provider::Pan115,
        list_type,
        share_code,
        receive_code,
        share_title,
        share_state,
        share_state_label,
        file_count,
        payload,
    })
}

async fn collapse_share_root_if_needed(
    client: &Client,
    endpoint: &str,
    share_code: &str,
    receive_code: Option<&str>,
    share_title: &str,
    entries: Vec<ShareEntry>,
) -> Result<(String, Vec<ShareEntry>, String), ShareListError> {
    if let [entry] = entries.as_slice() {
        if entry.is_folder() && entry.name == share_title {
            let root_cid = entry.cid.clone().unwrap_or_default();
            let listing =
                fetch_folder_listing(client, endpoint, share_code, receive_code, &root_cid).await?;
            return Ok((root_cid, listing.entries, share_title.to_string()));
        }
    }

    Ok(("0".to_string(), entries, String::new()))
}

async fn collect_flat_files(
    client: &Client,
    endpoint: &str,
    share_code: &str,
    receive_code: Option<&str>,
    root_entries: Vec<ShareEntry>,
    root_path: String,
) -> Result<Vec<Pan115ShareFile>, ShareListError> {
    let mut files = Vec::new();
    let mut stack = Vec::new();

    visit_entries_for_flat(root_entries, &root_path, &mut files, &mut stack);

    while let Some((cid, path)) = stack.pop() {
        let listing =
            fetch_folder_listing(client, endpoint, share_code, receive_code, &cid).await?;
        visit_entries_for_flat(listing.entries, &path, &mut files, &mut stack);
    }

    Ok(files)
}

fn visit_entries_for_flat(
    entries: Vec<ShareEntry>,
    base_path: &str,
    files: &mut Vec<Pan115ShareFile>,
    stack: &mut Vec<(String, String)>,
) {
    for entry in entries {
        let path = join_path(base_path, &entry.name);

        if entry.is_folder() {
            if let Some(cid) = entry.cid {
                stack.push((cid, path));
            }
            continue;
        }

        files.push(Pan115ShareFile {
            fid: entry.fid.unwrap_or_default(),
            parent_cid: entry.parent_cid.unwrap_or_default(),
            name: entry.name,
            path,
            size: entry.size,
            extension: entry.extension.filter(|value| !value.is_empty()),
        });
    }
}

fn build_tree_children<'a>(
    client: &'a Client,
    endpoint: &'a str,
    share_code: &'a str,
    receive_code: Option<&'a str>,
    entries: Vec<ShareEntry>,
    base_path: String,
) -> Pin<Box<dyn Future<Output = Result<Vec<Pan115ShareNode>, ShareListError>> + Send + 'a>> {
    Box::pin(async move {
        let mut children = Vec::new();

        for entry in entries {
            let path = join_path(&base_path, &entry.name);

            if entry.is_folder() {
                let cid = entry.cid.clone().unwrap_or_default();
                let listing =
                    fetch_folder_listing(client, endpoint, share_code, receive_code, &cid).await?;
                let nested_children = build_tree_children(
                    client,
                    endpoint,
                    share_code,
                    receive_code,
                    listing.entries,
                    path.clone(),
                )
                .await?;

                children.push(Pan115ShareNode::Folder {
                    cid,
                    name: entry.name,
                    path,
                    children: nested_children,
                });
                continue;
            }

            children.push(Pan115ShareNode::File {
                fid: entry.fid.unwrap_or_default(),
                parent_cid: entry.parent_cid.unwrap_or_default(),
                name: entry.name,
                path,
                size: entry.size,
                extension: entry.extension.filter(|value| !value.is_empty()),
            });
        }

        Ok(children)
    })
}

fn count_tree_files(nodes: &[Pan115ShareNode]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Pan115ShareNode::File { .. } => 1,
            Pan115ShareNode::Folder { children, .. } => count_tree_files(children),
        })
        .sum()
}

async fn fetch_folder_listing(
    client: &Client,
    endpoint: &str,
    share_code: &str,
    receive_code: Option<&str>,
    cid: &str,
) -> Result<FolderListing, ShareListError> {
    let mut offset = 0usize;
    let mut entries = Vec::new();
    let mut share_title = None;
    let mut share_state = None;
    let mut expected_count = None;

    loop {
        let response =
            fetch_share_snap_page(client, endpoint, share_code, receive_code, cid, offset).await?;

        let data = response.data.ok_or_else(|| {
            ShareListError::Api("115 share list response is missing data".to_string())
        })?;

        if share_title.is_none() {
            share_title = data
                .shareinfo
                .as_ref()
                .map(|shareinfo| shareinfo.share_title.clone());
        }
        if share_state.is_none() {
            share_state = data.share_state();
        }
        if expected_count.is_none() {
            expected_count = data.count;
        }

        let page_len = data.list.len();
        entries.extend(data.list.into_iter().map(ShareEntry::from));

        let total = expected_count.unwrap_or(entries.len());
        if page_len == 0 || entries.len() >= total {
            break;
        }

        offset += page_len;
    }

    Ok(FolderListing {
        entries,
        share_title,
        share_state,
    })
}

async fn fetch_share_snap_page(
    client: &Client,
    endpoint: &str,
    share_code: &str,
    receive_code: Option<&str>,
    cid: &str,
    offset: usize,
) -> Result<ShareSnapResponse, ShareListError> {
    let offset_string = offset.to_string();
    let limit_string = SHARE_LIST_PAGE_LIMIT.to_string();
    let mut query_params = vec![
        ("share_code".to_string(), share_code.to_string()),
        ("cid".to_string(), cid.to_string()),
        ("offset".to_string(), offset_string),
        ("limit".to_string(), limit_string),
        ("format".to_string(), "json".to_string()),
    ];

    if let Some(receive_code) = receive_code {
        query_params.push(("receive_code".to_string(), receive_code.to_string()));
    }

    let request_url = Url::parse_with_params(endpoint, &query_params)
        .map_err(|_| ShareListError::RequestFailed("invalid share list endpoint".to_string()))?;
    let response = with_share_snap_headers(client.get(request_url))
        .send()
        .await
        .map_err(map_request_error)?;

    response
        .json::<ShareSnapResponse>()
        .await
        .map_err(|error| ShareListError::ParseFailed(error.to_string()))
        .and_then(classify_list_response)
}

fn map_request_error(error: reqwest::Error) -> ShareListError {
    if error.is_timeout() {
        ShareListError::RequestFailed("115 share list request timed out".to_string())
    } else if error.is_connect() {
        ShareListError::RequestFailed("115 share list connection failed".to_string())
    } else {
        ShareListError::RequestFailed(error.to_string())
    }
}

fn classify_list_response(
    response: ShareSnapResponse,
) -> Result<ShareSnapResponse, ShareListError> {
    if response.state {
        return Ok(response);
    }

    match response.errno {
        ERRNO_MISSING_RECEIVE_CODE => Err(ShareListError::MissingReceiveCode),
        ERRNO_INVALID_RECEIVE_CODE => Err(ShareListError::InvalidReceiveCode),
        ERRNO_INVALID_SHARE_CODE => Err(ShareListError::InvalidShareCode),
        _ => Err(ShareListError::Api(if response.error.is_empty() {
            format!("errno={}", response.errno)
        } else {
            response.error
        })),
    }
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

fn join_path(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
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

struct FolderListing {
    entries: Vec<ShareEntry>,
    share_title: Option<String>,
    share_state: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ShareSnapResponse {
    state: bool,
    #[serde(default)]
    error: String,
    errno: i64,
    #[serde(default)]
    data: Option<ShareSnapData>,
}

#[derive(Debug, Deserialize)]
struct ShareSnapData {
    #[serde(default)]
    share_state: Option<i64>,
    #[serde(default)]
    count: Option<usize>,
    #[serde(default)]
    list: Vec<ShareListEntry>,
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
}

#[derive(Debug, Deserialize)]
struct ShareListEntry {
    #[serde(default)]
    fid: Option<ShareIdentifier>,
    #[serde(default)]
    cid: Option<ShareIdentifier>,
    #[serde(default)]
    pid: Option<ShareIdentifier>,
    #[serde(default)]
    n: String,
    #[serde(default)]
    s: u64,
    #[serde(default)]
    fc: i64,
    #[serde(default)]
    ico: Option<String>,
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
    fid: Option<String>,
    cid: Option<String>,
    parent_cid: Option<String>,
    name: String,
    size: u64,
    extension: Option<String>,
    file_category: i64,
}

impl ShareEntry {
    fn is_folder(&self) -> bool {
        self.file_category == 0 && self.fid.is_none()
    }
}

impl From<ShareListEntry> for ShareEntry {
    fn from(value: ShareListEntry) -> Self {
        let cid = value.cid.map(ShareIdentifier::into_string);
        let parent_cid = value
            .pid
            .map(ShareIdentifier::into_string)
            .or_else(|| cid.clone());

        Self {
            fid: value.fid.map(ShareIdentifier::into_string),
            cid,
            parent_cid,
            name: value.n,
            size: value.s,
            extension: value.ico,
            file_category: value.fc,
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        extract::{Query, State},
        http::HeaderMap,
        routing::get,
    };
    use serde_json::{Value, json};
    use std::{
        collections::BTreeMap,
        net::Ipv4Addr,
        sync::{Arc, Mutex},
    };
    use tokio::net::TcpListener;

    use crate::{
        checker::{
            Pan115ListType, Pan115ShareListPayload, Pan115ShareListRequest, Provider,
            service::LinkCheckerService,
        },
        error::ShareListError,
    };

    #[tokio::test]
    async fn lists_nested_share_files_in_files_mode() {
        let endpoint = spawn_share_api(mock_nested_share_responses()).await;
        let service = build_service();

        let response = service
            .list_pan115_share_with_endpoint(
                Pan115ShareListRequest {
                    url: "https://115cdn.com/s/swfsfjg3h7i?password=l3a6".to_string(),
                    list_type: Pan115ListType::Files,
                },
                &endpoint,
            )
            .await
            .unwrap();

        assert_eq!(response.provider, Provider::Pan115);
        assert_eq!(response.file_count, 3);
        assert_eq!(response.share_title.as_deref(), Some("记录的地平线"));

        let Pan115ShareListPayload::Files { files } = response.payload else {
            panic!("expected flat file payload");
        };

        let paths = files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                "记录的地平线/Season 1/Episode 01.mkv",
                "记录的地平线/Season 1/Episode 02.mkv",
                "记录的地平线/Season 2/Episode 01.mkv",
            ]
        );
    }

    #[tokio::test]
    async fn lists_nested_share_files_in_tree_mode() {
        let endpoint = spawn_share_api(mock_nested_share_responses()).await;
        let service = build_service();

        let response = service
            .list_pan115_share_with_endpoint(
                Pan115ShareListRequest {
                    url: "https://115cdn.com/s/swfsfjg3h7i?password=l3a6".to_string(),
                    list_type: Pan115ListType::Tree,
                },
                &endpoint,
            )
            .await
            .unwrap();

        let Pan115ShareListPayload::Tree { tree } = response.payload else {
            panic!("expected tree payload");
        };

        assert_eq!(tree.name, "记录的地平线");
        assert_eq!(tree.path, "记录的地平线");
        assert_eq!(tree.children.len(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn lists_root_level_multi_file_share_without_summary_title_path_prefix() {
        let endpoint = spawn_share_api(mock_root_level_multi_file_responses()).await;
        let original_url = "https://115cdn.com/s/swsh7q83fwl?password=de72".to_string();
        let context = build_direct_context(&original_url);

        let response = super::list_share_with_endpoint(context, Pan115ListType::Files, &endpoint)
            .await
            .unwrap();

        assert_eq!(
            response.share_title.as_deref(),
            Some("Episode 01.mkv等2个文件")
        );

        let Pan115ShareListPayload::Files { files } = response.payload else {
            panic!("expected flat file payload");
        };
        let paths = files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(paths, vec!["Episode 01.mkv", "Episode 02.mkv"]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn builds_root_level_multi_file_tree_without_summary_title_child_prefix() {
        let endpoint = spawn_share_api(mock_root_level_multi_file_responses()).await;
        let original_url = "https://115cdn.com/s/swsh7q83fwl?password=de72".to_string();
        let context = build_direct_context(&original_url);

        let response = super::list_share_with_endpoint(context, Pan115ListType::Tree, &endpoint)
            .await
            .unwrap();

        let Pan115ShareListPayload::Tree { tree } = response.payload else {
            panic!("expected tree payload");
        };
        assert_eq!(tree.name, "Episode 01.mkv等2个文件");
        assert_eq!(tree.path, "");

        let paths = tree
            .children
            .iter()
            .map(|node| match node {
                crate::checker::Pan115ShareNode::File { path, .. } => path.as_str(),
                crate::checker::Pan115ShareNode::Folder { .. } => panic!("expected only files"),
            })
            .collect::<Vec<_>>();

        assert_eq!(paths, vec!["Episode 01.mkv", "Episode 02.mkv"]);
    }

    #[tokio::test]
    async fn parses_live_style_numeric_folder_identifiers() {
        let endpoint = spawn_share_api(mock_live_style_numeric_responses()).await;
        let service = build_service();

        let response = service
            .list_pan115_share_with_endpoint(
                Pan115ShareListRequest {
                    url: "https://115cdn.com/s/swfsfjg3h7i?password=l3a6".to_string(),
                    list_type: Pan115ListType::Files,
                },
                &endpoint,
            )
            .await
            .unwrap();

        let Pan115ShareListPayload::Files { files } = response.payload else {
            panic!("expected flat file payload");
        };

        assert_eq!(files.len(), 2);
        assert_eq!(files[0].parent_cid, "3332132737553719669");
        assert_eq!(files[0].path, "记录的地平线/Episode 01.mkv");
    }

    #[tokio::test]
    async fn normalizes_anxia_share_url_to_115cdn() {
        let endpoint = spawn_share_api(mock_nested_share_responses()).await;
        let service = build_service();

        let response = service
            .list_pan115_share_with_endpoint(
                Pan115ShareListRequest {
                    url: "https://anxia.com/s/swfsfjg3h7i?password=l3a6#".to_string(),
                    list_type: Pan115ListType::Files,
                },
                &endpoint,
            )
            .await
            .unwrap();

        assert_eq!(
            response.normalized_url,
            "https://115cdn.com/s/swfsfjg3h7i?password=l3a6"
        );
    }

    #[tokio::test]
    async fn sends_115_origin_headers_with_random_forwarded_ip() {
        let captured_headers = Arc::new(Mutex::new(Vec::new()));
        let endpoint = spawn_share_api_with_header_capture(
            BTreeMap::from([(
                key("0", 0),
                json!({
                    "state": true,
                    "error": "",
                    "errno": 0,
                    "data": {
                        "shareinfo": {
                            "share_state": 1,
                            "share_title": "空分享",
                            "forbid_reason": "",
                            "file_size": 0,
                            "has_receive_code": 1,
                            "have_vio_file": 0,
                            "share_duration": -1,
                            "expire_time": -1
                        },
                        "count": 0,
                        "list": []
                    }
                }),
            )]),
            captured_headers.clone(),
        )
        .await;
        let service = build_service();

        service
            .list_pan115_share_with_endpoint(
                Pan115ShareListRequest {
                    url: "https://115cdn.com/s/swfsfjg3h7i?password=l3a6".to_string(),
                    list_type: Pan115ListType::Files,
                },
                &endpoint,
            )
            .await
            .unwrap();

        let headers = captured_headers.lock().unwrap();
        let first_request = headers.first().expect("expected one upstream request");

        assert_eq!(first_request.referer.as_deref(), Some("https://115.com/"));
        assert_eq!(first_request.origin.as_deref(), Some("https://115.com"));
        assert!(
            first_request
                .x_forwarded_for
                .as_deref()
                .is_some_and(is_public_forwarded_ipv4),
            "expected a public IPv4 X-Forwarded-For header, got {:?}",
            first_request.x_forwarded_for
        );
    }

    #[tokio::test]
    async fn returns_missing_receive_code_error_when_share_requires_password() {
        let endpoint = spawn_share_api(BTreeMap::from([(
            key("0", 0),
            json!({
                "state": false,
                "error": "请输入访问码",
                "errno": 4100012,
                "data": {
                    "is_access": 0
                }
            }),
        )]))
        .await;
        let service = build_service();

        let error = service
            .list_pan115_share_with_endpoint(
                Pan115ShareListRequest {
                    url: "https://115cdn.com/s/swfsfjg3h7i".to_string(),
                    list_type: Pan115ListType::Files,
                },
                &endpoint,
            )
            .await
            .unwrap_err();

        assert!(matches!(error, ShareListError::MissingReceiveCode));
    }

    fn build_service() -> LinkCheckerService {
        LinkCheckerService::new_without_proxy(std::time::Duration::from_secs(10)).unwrap()
    }

    fn build_direct_context(original_url: &str) -> crate::providers::common::CheckContext {
        crate::providers::common::CheckContext {
            original_url: original_url.to_string(),
            url: url::Url::parse(original_url).unwrap(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .no_proxy()
                .build()
                .unwrap(),
        }
    }

    fn mock_nested_share_responses() -> BTreeMap<String, Value> {
        BTreeMap::from([
            (
                key("0", 0),
                json!({
                    "state": true,
                    "error": "",
                    "errno": 0,
                    "data": {
                        "shareinfo": {
                            "share_state": 1,
                            "share_title": "记录的地平线",
                            "forbid_reason": "",
                            "file_size": 3,
                            "has_receive_code": 1,
                            "have_vio_file": 0,
                            "share_duration": -1,
                            "expire_time": -1
                        },
                        "count": 1,
                        "list": [
                            {
                                "cid": "root-folder",
                                "pid": "0",
                                "n": "记录的地平线",
                                "s": 0,
                                "fc": 0,
                                "t": "1767759819",
                                "fl": []
                            }
                        ]
                    }
                }),
            ),
            (
                key("root-folder", 0),
                json!({
                    "state": true,
                    "error": "",
                    "errno": 0,
                    "data": {
                        "shareinfo": {
                            "share_state": 1,
                            "share_title": "记录的地平线",
                            "forbid_reason": "",
                            "file_size": 3,
                            "has_receive_code": 1,
                            "have_vio_file": 0,
                            "share_duration": -1,
                            "expire_time": -1
                        },
                        "count": 2,
                        "list": [
                            {
                                "cid": "season-1",
                                "pid": "root-folder",
                                "n": "Season 1",
                                "s": 0,
                                "fc": 0,
                                "t": "1767757808",
                                "fl": []
                            },
                            {
                                "cid": "season-2",
                                "pid": "root-folder",
                                "n": "Season 2",
                                "s": 0,
                                "fc": 0,
                                "t": "1767757809",
                                "fl": []
                            }
                        ]
                    }
                }),
            ),
            (
                key("season-1", 0),
                json!({
                    "state": true,
                    "error": "",
                    "errno": 0,
                    "data": {
                        "shareinfo": {
                            "share_state": 1,
                            "share_title": "记录的地平线",
                            "forbid_reason": "",
                            "file_size": 3,
                            "has_receive_code": 1,
                            "have_vio_file": 0,
                            "share_duration": -1,
                            "expire_time": -1
                        },
                        "count": 2,
                        "list": [
                            {
                                "fid": "file-1",
                                "cid": "season-1",
                                "n": "Episode 01.mkv",
                                "s": 100,
                                "fc": 1,
                                "sha": "sha1-1",
                                "ico": "mkv",
                                "t": "1767257089",
                                "fl": []
                            },
                            {
                                "fid": "file-2",
                                "cid": "season-1",
                                "n": "Episode 02.mkv",
                                "s": 200,
                                "fc": 1,
                                "sha": "sha1-2",
                                "ico": "mkv",
                                "t": "1767257090",
                                "fl": []
                            }
                        ]
                    }
                }),
            ),
            (
                key("season-2", 0),
                json!({
                    "state": true,
                    "error": "",
                    "errno": 0,
                    "data": {
                        "shareinfo": {
                            "share_state": 1,
                            "share_title": "记录的地平线",
                            "forbid_reason": "",
                            "file_size": 3,
                            "has_receive_code": 1,
                            "have_vio_file": 0,
                            "share_duration": -1,
                            "expire_time": -1
                        },
                        "count": 1,
                        "list": [
                            {
                                "fid": "file-3",
                                "cid": "season-2",
                                "n": "Episode 01.mkv",
                                "s": 300,
                                "fc": 1,
                                "sha": "sha1-3",
                                "ico": "mkv",
                                "t": "1767257091",
                                "fl": []
                            }
                        ]
                    }
                }),
            ),
        ])
    }

    fn mock_live_style_numeric_responses() -> BTreeMap<String, Value> {
        BTreeMap::from([
            (
                key("0", 0),
                json!({
                    "state": true,
                    "error": "",
                    "errno": 0,
                    "data": {
                        "shareinfo": {
                            "share_state": 1,
                            "share_title": "记录的地平线",
                            "forbid_reason": "",
                            "file_size": 2,
                            "has_receive_code": 1,
                            "have_vio_file": 0,
                            "share_duration": -1,
                            "expire_time": -1
                        },
                        "count": 1,
                        "list": [
                            {
                                "cid": "3332132737553719669",
                                "pid": "0",
                                "n": "记录的地平线",
                                "s": 0,
                                "fc": 0,
                                "t": "1767759819",
                                "fl": []
                            }
                        ]
                    }
                }),
            ),
            (
                key("3332132737553719669", 0),
                json!({
                    "state": true,
                    "error": "",
                    "errno": 0,
                    "data": {
                        "shareinfo": {
                            "share_state": 1,
                            "share_title": "记录的地平线",
                            "forbid_reason": "",
                            "file_size": 2,
                            "has_receive_code": 1,
                            "have_vio_file": 0,
                            "share_duration": -1,
                            "expire_time": -1
                        },
                        "count": 2,
                        "list": [
                            {
                                "fid": "3332299784224957410",
                                "uid": 325039613,
                                "cid": 3332132737553719669i64,
                                "n": "Episode 01.mkv",
                                "s": 100,
                                "fc": 1,
                                "sha": "sha1-1",
                                "ico": "mkv",
                                "t": "1767257089",
                                "fl": []
                            },
                            {
                                "fid": "3332293604345831549",
                                "uid": 325039613,
                                "cid": 3332132737553719669i64,
                                "n": "Episode 02.mkv",
                                "s": 200,
                                "fc": 1,
                                "sha": "sha1-2",
                                "ico": "mkv",
                                "t": "1767257090",
                                "fl": []
                            }
                        ]
                    }
                }),
            ),
        ])
    }

    fn mock_root_level_multi_file_responses() -> BTreeMap<String, Value> {
        BTreeMap::from([(
            key("0", 0),
            json!({
                "state": true,
                "error": "",
                "errno": 0,
                "data": {
                    "shareinfo": {
                        "share_state": 1,
                        "share_title": "Episode 01.mkv等2个文件",
                        "forbid_reason": "",
                        "file_size": 2,
                        "has_receive_code": 1,
                        "have_vio_file": 0,
                        "share_duration": -1,
                        "expire_time": -1
                    },
                    "count": 2,
                    "list": [
                        {
                            "fid": "file-1",
                            "cid": 0,
                            "n": "Episode 01.mkv",
                            "s": 100,
                            "fc": 1,
                            "sha": "sha1-1",
                            "ico": "mkv",
                            "t": "1767257089",
                            "fl": []
                        },
                        {
                            "fid": "file-2",
                            "cid": 0,
                            "n": "Episode 02.mkv",
                            "s": 200,
                            "fc": 1,
                            "sha": "sha1-2",
                            "ico": "mkv",
                            "t": "1767257090",
                            "fl": []
                        }
                    ]
                }
            }),
        )])
    }

    fn key(cid: &str, offset: usize) -> String {
        format!("{cid}:{offset}")
    }

    #[derive(Debug)]
    struct CapturedHeaders {
        referer: Option<String>,
        origin: Option<String>,
        x_forwarded_for: Option<String>,
    }

    impl CapturedHeaders {
        fn from_headers(headers: &HeaderMap) -> Self {
            Self {
                referer: header_value(headers, "referer"),
                origin: header_value(headers, "origin"),
                x_forwarded_for: header_value(headers, "x-forwarded-for"),
            }
        }
    }

    fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(ToString::to_string)
    }

    fn is_public_forwarded_ipv4(value: &str) -> bool {
        let Ok(address) = value.parse::<Ipv4Addr>() else {
            return false;
        };
        let [first, second, ..] = address.octets();

        if !(1..=223).contains(&first) {
            return false;
        }

        !matches!(
            (first, second),
            (10, _) | (127, _) | (172, 16..=31) | (192, 168) | (169, 254)
        )
    }

    async fn spawn_share_api(responses: BTreeMap<String, Value>) -> String {
        spawn_share_api_with_header_capture(responses, Arc::new(Mutex::new(Vec::new()))).await
    }

    async fn spawn_share_api_with_header_capture(
        responses: BTreeMap<String, Value>,
        captured_headers: Arc<Mutex<Vec<CapturedHeaders>>>,
    ) -> String {
        #[derive(Clone)]
        struct MockState {
            responses: Arc<BTreeMap<String, Value>>,
            captured_headers: Arc<Mutex<Vec<CapturedHeaders>>>,
        }

        async fn share_snap(
            State(state): State<MockState>,
            headers: HeaderMap,
            Query(query): Query<BTreeMap<String, String>>,
        ) -> Json<Value> {
            state
                .captured_headers
                .lock()
                .unwrap()
                .push(CapturedHeaders::from_headers(&headers));

            let cid = query.get("cid").cloned().unwrap_or_else(|| "0".to_string());
            let offset = query
                .get("offset")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or_default();
            let key = format!("{cid}:{offset}");

            Json(state.responses.get(&key).cloned().unwrap_or_else(
                || json!({"state": false, "error": "missing mock response", "errno": 999999}),
            ))
        }

        let app = Router::new()
            .route("/share/snap", get(share_snap))
            .with_state(MockState {
                responses: Arc::new(responses),
                captured_headers,
            });

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{address}/share/snap")
    }
}
