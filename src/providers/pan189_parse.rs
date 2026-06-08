use url::Url;

pub(crate) fn extract_share_code(url: &Url) -> Option<String> {
    let share_code = parse_share_code(&extract_raw_share_token(url)?);

    if share_code.is_empty() {
        None
    } else {
        Some(share_code)
    }
}

pub(crate) fn extract_access_code(url: &Url) -> Option<String> {
    extract_access_code_from_query(url).or_else(|| {
        extract_raw_share_token(url).and_then(|raw| extract_embedded_access_code(&raw))
    })
}

pub(crate) fn normalize_share_url(share_code: &str, access_code: Option<&str>) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_share_code_from_short_path() {
        let url = Url::parse("https://cloud.189.cn/t/yYvIvyVfY7rm?accessCode=1hit").unwrap();

        assert_eq!(extract_share_code(&url).as_deref(), Some("yYvIvyVfY7rm"));
    }

    #[test]
    fn extracts_share_code_from_web_share_query() {
        let url = Url::parse("https://cloud.189.cn/web/share?code=nIB7Fr6Nn2ua").unwrap();

        assert_eq!(extract_share_code(&url).as_deref(), Some("nIB7Fr6Nn2ua"));
    }

    #[test]
    fn extracts_embedded_access_code_from_code_query() {
        let url = Url::parse(
            "https://cloud.189.cn/web/share?code=UreieiIZJbU3%EF%BC%88%E8%AE%BF%E9%97%AE%E7%A0%81%EF%BC%9Axw6v%EF%BC%89",
        )
        .unwrap();

        assert_eq!(extract_share_code(&url).as_deref(), Some("UreieiIZJbU3"));
        assert_eq!(extract_access_code(&url).as_deref(), Some("xw6v"));
    }
}
