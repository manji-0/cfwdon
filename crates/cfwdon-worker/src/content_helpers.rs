use super::{AppConfig, MastodonTagHistoryEntry, StatusRow, instance_base_url, instance_host};
use cfwdon_domain::AccountHandle;
use std::collections::HashSet;

pub(crate) fn status_contains_tag(status: &StatusRow, tag: &str) -> bool {
    let normalized_tag = tag.trim().trim_start_matches('#').to_ascii_lowercase();
    if normalized_tag.is_empty() {
        return true;
    }

    let needle = format!("#{normalized_tag}");
    status.text.to_ascii_lowercase().contains(&needle)
}

pub(crate) fn extract_hashtags_from_text(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut seen = HashSet::new();
    let chars = text.chars().collect::<Vec<_>>();
    let mut index = 0usize;

    while index < chars.len() {
        if chars[index] != '#' {
            index += 1;
            continue;
        }
        if index > 0 {
            let previous = chars[index - 1];
            if previous.is_ascii_alphanumeric() || previous == '_' {
                index += 1;
                continue;
            }
        }

        let mut end = index + 1;
        while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
            end += 1;
        }
        if end == index + 1 {
            index += 1;
            continue;
        }

        let tag = chars[index + 1..end]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase();
        if seen.insert(tag.clone()) {
            tags.push(tag);
        }
        index = end;
    }

    tags
}

pub(crate) fn extract_mentions_from_text(text: &str, config: &AppConfig) -> Vec<AccountHandle> {
    extract_account_handles_from_text(text, config)
        .into_iter()
        .filter(|handle| handle.is_local_to(&config.instance_domain))
        .collect()
}

pub(crate) fn extract_account_handles_from_text(
    text: &str,
    config: &AppConfig,
) -> Vec<AccountHandle> {
    let mut mentions = Vec::new();
    let mut seen = HashSet::new();
    let chars = text.chars().collect::<Vec<_>>();
    let mut index = 0usize;

    while index < chars.len() {
        if chars[index] != '@' {
            index += 1;
            continue;
        }
        if index > 0 {
            let previous = chars[index - 1];
            if previous.is_ascii_alphanumeric() || previous == '_' {
                index += 1;
                continue;
            }
        }

        let mut end = index + 1;
        while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
            end += 1;
        }
        if end == index + 1 {
            index += 1;
            continue;
        }

        let username = chars[index + 1..end]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase();
        let mut domain = None;
        let mut next_index = end;
        if next_index < chars.len() && chars[next_index] == '@' {
            let mut domain_end = next_index + 1;
            while domain_end < chars.len()
                && (chars[domain_end].is_ascii_alphanumeric()
                    || chars[domain_end] == '-'
                    || chars[domain_end] == '.')
            {
                domain_end += 1;
            }
            if domain_end > next_index + 1 {
                domain = Some(
                    chars[next_index + 1..domain_end]
                        .iter()
                        .collect::<String>()
                        .to_ascii_lowercase(),
                );
                next_index = domain_end;
            }
        }

        let handle = AccountHandle {
            username,
            domain: domain.or_else(|| Some(instance_host(config))),
        };
        let key = format!(
            "{}@{}",
            handle.username,
            handle
                .domain
                .clone()
                .unwrap_or_else(|| instance_host(config))
        );
        if seen.insert(key) {
            mentions.push(handle);
        }
        index = next_index;
    }

    mentions
}

pub(crate) fn strip_html_tags(html: &str) -> String {
    let mut output = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
}

/// Convert untrusted federated HTML into safe, escaped paragraph markup.
///
/// Mastodon-compatible clients still accept plain `<p>`/`<br>` bodies; stripping
/// remote tags removes stored XSS while preserving readable text.
pub(crate) fn sanitize_remote_status_html(html: &str) -> String {
    let plain = decode_basic_html_entities(&strip_html_tags(html));
    crate::render_status_html(&plain)
}

pub(crate) fn sanitize_remote_plain_text(value: &str) -> String {
    decode_basic_html_entities(&strip_html_tags(value))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn sanitize_remote_http_url(value: Option<&str>) -> Option<String> {
    let value = value.map(str::trim).filter(|value| !value.is_empty())?;
    if !cfwdon_domain::remote_http_url_scheme_allowed(value) {
        return None;
    }
    Some(value.to_owned())
}

fn decode_basic_html_entities(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub(crate) fn extract_hashtags_from_html(html: &str) -> Vec<String> {
    extract_hashtags_from_text(&strip_html_tags(html))
}

pub(crate) fn tag_rest_id(name: &str) -> String {
    let checksum = name.bytes().fold(0u32, |acc, byte| {
        acc.wrapping_mul(16777619).wrapping_add(byte as u32)
    });
    checksum.to_string()
}

pub(crate) fn tag_url(config: &AppConfig, name: &str) -> String {
    format!("{}/tags/{}", instance_base_url(config), name)
}

#[cfg(test)]
mod tests {
    use super::{
        sanitize_remote_http_url, sanitize_remote_plain_text, sanitize_remote_status_html,
    };

    #[test]
    fn sanitize_remote_status_html_strips_script_and_event_handlers() {
        let html = sanitize_remote_status_html(
            r#"<p onclick="alert(1)">hi <script>alert(2)</script><a href="javascript:alert(3)">x</a></p>"#,
        );
        assert!(!html.to_ascii_lowercase().contains("<script"));
        assert!(!html.to_ascii_lowercase().contains("onclick"));
        assert!(!html.to_ascii_lowercase().contains("javascript:"));
        assert!(html.contains("hi"));
        assert!(html.contains("x"));
    }

    #[test]
    fn sanitize_remote_plain_text_strips_markup() {
        assert_eq!(
            sanitize_remote_plain_text("  hello <b>world</b>  "),
            "hello world"
        );
    }

    #[test]
    fn sanitize_remote_http_url_rejects_non_http_schemes() {
        assert_eq!(
            sanitize_remote_http_url(Some("https://remote.example/note")),
            Some("https://remote.example/note".to_owned())
        );
        assert_eq!(sanitize_remote_http_url(Some("javascript:alert(1)")), None);
        assert_eq!(sanitize_remote_http_url(Some("")), None);
    }
}

pub(crate) fn tag_history_stub() -> Vec<MastodonTagHistoryEntry> {
    Vec::new()
}
