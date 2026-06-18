use std::{future::Future, pin::Pin};

use reqwest::Client;

use crate::{
    checker::{
        Pan189ListType, Pan189ShareFile, Pan189ShareFolderNode, Pan189ShareListPayload,
        Pan189ShareListResponse, Pan189ShareNode, Provider,
    },
    error::ShareListError,
    providers::{
        common::CheckContext,
        pan189_api::{
            ListShareDirBody, ShareFileEntry, ShareFolderEntry, ShareSession, list_share_dir,
            resolve_share_session,
        },
        pan189_parse::{extract_access_code, extract_share_code, normalize_share_url},
    },
};

const SHARE_LIST_PAGE_SIZE: i64 = 100;

pub async fn list_share(
    context: CheckContext,
    list_type: Pan189ListType,
) -> Result<Pan189ShareListResponse, ShareListError> {
    list_share_with_endpoints(context, list_type, None).await
}

pub(crate) async fn list_share_with_endpoints(
    context: CheckContext,
    list_type: Pan189ListType,
    list_endpoint: Option<&str>,
) -> Result<Pan189ShareListResponse, ShareListError> {
    let link = parse_share_link(&context.url)?;
    let share_code = link.share_code.as_str();
    let session =
        resolve_share_session(&context.client, share_code, link.access_code.as_deref()).await?;

    let share_name = if session.file_name.is_empty() {
        None
    } else {
        Some(session.file_name.clone())
    };
    let root_name = share_name.clone().unwrap_or_else(|| share_code.to_string());
    let root_listing = fetch_folder_listing(
        &context.client,
        list_endpoint,
        &share_code,
        &session,
        &session.file_id,
    )
    .await?;

    let (root_file_id, root_entries) = collapse_share_root_if_needed(
        &context.client,
        list_endpoint,
        &share_code,
        &session,
        &root_name,
        root_listing,
    )
    .await?;
    let root_path = root_name.clone();

    let payload = match list_type {
        Pan189ListType::Files => {
            let mut files = collect_flat_files(
                &context.client,
                list_endpoint,
                &share_code,
                &session,
                root_entries,
                root_path,
            )
            .await?;
            files.sort_by(|left, right| left.path.cmp(&right.path));
            Pan189ShareListPayload::Files { files }
        }
        Pan189ListType::Tree => {
            let children = build_tree_children(
                &context.client,
                list_endpoint,
                &share_code,
                &session,
                root_entries,
                root_path.clone(),
            )
            .await?;
            Pan189ShareListPayload::Tree {
                tree: Pan189ShareFolderNode {
                    file_id: root_file_id,
                    name: root_name,
                    path: root_path,
                    children,
                },
            }
        }
    };

    let file_count = match &payload {
        Pan189ShareListPayload::Files { files } => files.len(),
        Pan189ShareListPayload::Tree { tree } => count_tree_files(&tree.children),
    };

    Ok(Pan189ShareListResponse {
        original_url: context.original_url,
        normalized_url: link.normalized_url,
        provider: Provider::Pan189,
        list_type,
        share_code: link.share_code,
        access_code: if session.access_code.is_empty() {
            None
        } else {
            Some(session.access_code)
        },
        share_name,
        share_id: Some(session.share_id),
        file_count,
        payload,
    })
}

#[derive(Debug, Clone)]
struct ShareEntry {
    id: String,
    parent_id: String,
    name: String,
    size: u64,
    is_folder: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShareLink {
    share_code: String,
    access_code: Option<String>,
    normalized_url: String,
}

fn parse_share_link(url: &url::Url) -> Result<ShareLink, ShareListError> {
    let share_code = extract_share_code(url).ok_or(ShareListError::InvalidPan189ShareUrl)?;
    let access_code = extract_access_code(url);
    let normalized_url = normalize_share_url(&share_code, access_code.as_deref());

    Ok(ShareLink {
        share_code,
        access_code,
        normalized_url,
    })
}

async fn collapse_share_root_if_needed(
    client: &Client,
    list_endpoint: Option<&str>,
    share_code: &str,
    session: &ShareSession,
    share_name: &str,
    entries: Vec<ShareEntry>,
) -> Result<(String, Vec<ShareEntry>), ShareListError> {
    if let [entry] = entries.as_slice() {
        if entry.is_folder && entry.name == share_name {
            let listing =
                fetch_folder_listing(client, list_endpoint, share_code, session, &entry.id).await?;
            return Ok((entry.id.clone(), listing));
        }
    }

    Ok((session.file_id.clone(), entries))
}

async fn collect_flat_files(
    client: &Client,
    list_endpoint: Option<&str>,
    share_code: &str,
    session: &ShareSession,
    root_entries: Vec<ShareEntry>,
    root_path: String,
) -> Result<Vec<Pan189ShareFile>, ShareListError> {
    let mut files = Vec::new();
    let mut stack = Vec::new();

    visit_entries_for_flat(root_entries, &root_path, &mut files, &mut stack);

    while let Some((folder_id, path)) = stack.pop() {
        let listing =
            fetch_folder_listing(client, list_endpoint, share_code, session, &folder_id).await?;
        visit_entries_for_flat(listing, &path, &mut files, &mut stack);
    }

    Ok(files)
}

fn visit_entries_for_flat(
    entries: Vec<ShareEntry>,
    base_path: &str,
    files: &mut Vec<Pan189ShareFile>,
    stack: &mut Vec<(String, String)>,
) {
    for entry in entries {
        let path = join_path(base_path, &entry.name);

        if entry.is_folder {
            stack.push((entry.id, path));
            continue;
        }

        files.push(Pan189ShareFile {
            file_id: entry.id,
            parent_file_id: entry.parent_id,
            name: entry.name,
            path,
            size: entry.size,
        });
    }
}

fn build_tree_children<'a>(
    client: &'a Client,
    list_endpoint: Option<&'a str>,
    share_code: &'a str,
    session: &'a ShareSession,
    entries: Vec<ShareEntry>,
    base_path: String,
) -> Pin<Box<dyn Future<Output = Result<Vec<Pan189ShareNode>, ShareListError>> + Send + 'a>> {
    Box::pin(async move {
        let mut children = Vec::new();

        for entry in entries {
            let path = join_path(&base_path, &entry.name);

            if entry.is_folder {
                let listing =
                    fetch_folder_listing(client, list_endpoint, share_code, session, &entry.id)
                        .await?;
                let nested_children = build_tree_children(
                    client,
                    list_endpoint,
                    share_code,
                    session,
                    listing,
                    path.clone(),
                )
                .await?;

                children.push(Pan189ShareNode::Folder {
                    file_id: entry.id,
                    name: entry.name,
                    path,
                    children: nested_children,
                });
                continue;
            }

            children.push(Pan189ShareNode::File {
                file_id: entry.id,
                parent_file_id: entry.parent_id,
                name: entry.name,
                path,
                size: entry.size,
            });
        }

        Ok(children)
    })
}

fn count_tree_files(nodes: &[Pan189ShareNode]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Pan189ShareNode::File { .. } => 1,
            Pan189ShareNode::Folder { children, .. } => count_tree_files(children),
        })
        .sum()
}

async fn fetch_folder_listing(
    client: &Client,
    list_endpoint: Option<&str>,
    share_code: &str,
    session: &ShareSession,
    folder_id: &str,
) -> Result<Vec<ShareEntry>, ShareListError> {
    let mut page_num = 1_i64;
    let mut entries = Vec::new();

    loop {
        let body = if let Some(endpoint) = list_endpoint {
            fetch_list_page_with_endpoint(
                client, endpoint, share_code, session, folder_id, page_num,
            )
            .await?
        } else {
            list_share_dir(
                client,
                session,
                share_code,
                folder_id,
                page_num,
                SHARE_LIST_PAGE_SIZE,
            )
            .await?
        };

        let Some(file_list) = body.file_list else {
            break;
        };

        let page_len = file_list.files.len() + file_list.folders.len();
        entries.extend(file_list.files.into_iter().map(ShareEntry::from_file));
        entries.extend(file_list.folders.into_iter().map(ShareEntry::from_folder));

        if page_len == 0 {
            break;
        }

        let total = file_list.count.unwrap_or(entries.len() as i64);
        if entries.len() as i64 >= total {
            break;
        }

        page_num += 1;
    }

    Ok(entries)
}

async fn fetch_list_page_with_endpoint(
    client: &Client,
    endpoint: &str,
    share_code: &str,
    session: &ShareSession,
    folder_id: &str,
    page_num: i64,
) -> Result<ListShareDirBody, ShareListError> {
    let request_url = url::Url::parse_with_params(
        endpoint,
        &[
            ("returnType", "json"),
            ("pageNum", &page_num.to_string()),
            ("pageSize", &SHARE_LIST_PAGE_SIZE.to_string()),
            ("fileId", folder_id),
            ("shareDirFileId", folder_id),
            ("isFolder", "true"),
            ("shareId", &session.share_id.to_string()),
            ("shareMode", &session.share_mode.to_string()),
            ("iconOption", "5"),
            ("orderBy", "lastOpTime"),
            ("descending", "true"),
            ("accessCode", &session.access_code),
        ],
    )
    .expect("mock pan189 list endpoint should always be valid");

    let response =
        crate::providers::pan189_api::with_share_headers(client.get(request_url), share_code)
            .send()
            .await
            .map_err(|error| ShareListError::RequestFailed(error.to_string()))?;

    response
        .json::<ListShareDirBody>()
        .await
        .map_err(|error| ShareListError::ParseFailed(error.to_string()))
}

impl ShareEntry {
    fn from_file(value: ShareFileEntry) -> Self {
        Self {
            id: value.id,
            parent_id: value.parent_id,
            name: value.name,
            size: value.size,
            is_folder: false,
        }
    }

    fn from_folder(value: ShareFolderEntry) -> Self {
        Self {
            id: value.id,
            parent_id: value.parent_id,
            name: value.name,
            size: value.file_list_size,
            is_folder: true,
        }
    }
}

fn join_path(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    #[test]
    fn parses_h5_fragment_route_for_file_listing() {
        let url = Url::parse("https://h5.cloud.189.cn/share.html#/t/yYvIvyVfY7rm?accessCode=1hit")
            .unwrap();

        let link = parse_share_link(&url).unwrap();

        assert_eq!(link.share_code, "yYvIvyVfY7rm");
        assert_eq!(link.access_code.as_deref(), Some("1hit"));
        assert_eq!(
            link.normalized_url,
            "https://cloud.189.cn/t/yYvIvyVfY7rm?accessCode=1hit"
        );
    }
}
