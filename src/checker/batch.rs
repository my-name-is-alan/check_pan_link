use std::collections::HashSet;

use url::Url;

pub fn normalize_and_deduplicate_urls(urls: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for value in urls {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }

        let key = Url::parse(trimmed)
            .map(|url| url.to_string())
            .unwrap_or_else(|_| trimmed.to_string());

        if seen.insert(key.clone()) {
            normalized.push(key);
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_deduplicates_urls() {
        let urls = vec![
            " https://example.com/path ".to_string(),
            "https://example.com/path".to_string(),
            "not a url".to_string(),
            "not a url".to_string(),
        ];

        assert_eq!(
            normalize_and_deduplicate_urls(urls),
            vec![
                "https://example.com/path".to_string(),
                "not a url".to_string()
            ]
        );
    }
}
