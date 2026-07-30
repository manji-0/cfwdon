mod admin;
mod federated;
mod gif_static;
mod store;

pub(crate) use admin::{
    admin_create_custom_emoji_response, admin_custom_emojis_response,
    admin_delete_custom_emoji_response, admin_update_custom_emoji_response,
};
pub(crate) use federated::{
    FederatedEmojiMap, RemoteStatusFederatedEmojisPreload,
    extract_federated_emojis_from_activitypub_object, preload_remote_status_federated_emojis,
    resolve_account_emojis_from_document, resolve_status_emojis,
};
pub(crate) use store::{config_with_resolved_custom_emojis, resolve_custom_emojis};

use cfwdon_core::{AppConfig, CustomEmoji, is_custom_emoji_shortcode};
use cfwdon_domain::{PollDraft, StatusDraft};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

pub(crate) fn sanitize_emoji_shortcodes(text: &str, config: &AppConfig) -> String {
    let allowed = registered_shortcodes(config);
    strip_unregistered_emoji_shortcodes(text, &allowed)
}

pub(crate) fn custom_emojis_used_in_texts<'a>(
    texts: impl IntoIterator<Item = &'a str>,
    config: &AppConfig,
) -> Vec<serde_json::Value> {
    let registry = emoji_registry(config);
    let mut seen = HashSet::new();
    let mut emojis = Vec::new();

    for text in texts {
        for shortcode in extract_emoji_shortcodes(text) {
            if seen.insert(shortcode.clone())
                && let Some(emoji) = registry.get(&shortcode)
            {
                emojis.push(custom_emoji_to_json(emoji));
            }
        }
    }

    emojis
}

pub(crate) fn list_custom_emojis(config: &AppConfig) -> Vec<serde_json::Value> {
    config
        .custom_emojis
        .iter()
        .map(custom_emoji_to_json)
        .collect()
}

pub(crate) fn parse_custom_emojis_json(raw: &str) -> Result<Vec<CustomEmoji>, String> {
    let inputs = serde_json::from_str::<Vec<CustomEmojiInput>>(raw)
        .map_err(|error| format!("invalid CUSTOM_EMOJIS_JSON: {error}"))?;
    Ok(normalize_custom_emojis(inputs))
}

pub(super) fn custom_emoji_to_json(emoji: &CustomEmoji) -> serde_json::Value {
    serde_json::to_value(emoji).unwrap_or(serde_json::Value::Null)
}

#[derive(Debug, Deserialize)]
struct CustomEmojiInput {
    shortcode: String,
    url: String,
    static_url: Option<String>,
    visible_in_picker: Option<bool>,
    category: Option<String>,
}

fn normalize_custom_emojis(inputs: Vec<CustomEmojiInput>) -> Vec<CustomEmoji> {
    let mut seen = HashSet::new();
    let mut emojis = Vec::new();

    for input in inputs {
        let shortcode = input.shortcode.trim().to_owned();
        if shortcode.is_empty() || !is_custom_emoji_shortcode(&shortcode) {
            continue;
        }
        let url = input.url.trim().to_owned();
        if url.is_empty() {
            continue;
        }
        if !seen.insert(shortcode.clone()) {
            continue;
        }
        let static_url = input
            .static_url
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| url.clone());
        let category = input
            .category
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        emojis.push(CustomEmoji {
            shortcode,
            url,
            static_url,
            visible_in_picker: input.visible_in_picker.unwrap_or(true),
            category,
        });
    }

    emojis
}

pub(crate) fn sanitize_status_draft(
    draft: StatusDraft,
    config: &AppConfig,
) -> Result<StatusDraft, String> {
    let poll = match draft.poll() {
        Some(poll) => Some(sanitize_poll_draft(poll, config)?),
        None => None,
    };

    StatusDraft::try_from_persisted(
        sanitize_emoji_shortcodes(draft.text(), config),
        draft.visibility(),
        sanitize_emoji_shortcodes(draft.spoiler_text(), config),
        draft.sensitive(),
        draft.language().map(str::to_owned),
        draft.quote_approval_policy(),
        draft.in_reply_to_id().map(str::to_owned),
        draft.media_ids().to_vec(),
        poll,
    )
    .map_err(|error| error.to_string())
}

fn sanitize_poll_draft(poll: &PollDraft, config: &AppConfig) -> Result<PollDraft, String> {
    PollDraft::try_new(
        poll.options()
            .iter()
            .map(|option| sanitize_emoji_shortcodes(option, config))
            .collect(),
        poll.expires_in_seconds(),
        poll.multiple(),
        poll.hide_totals(),
    )
    .map_err(|error| error.to_string())
}

fn registered_shortcodes(config: &AppConfig) -> HashSet<String> {
    config
        .custom_emojis
        .iter()
        .map(|emoji| emoji.shortcode.clone())
        .collect()
}

fn emoji_registry(config: &AppConfig) -> HashMap<String, &CustomEmoji> {
    config
        .custom_emojis
        .iter()
        .map(|emoji| (emoji.shortcode.clone(), emoji))
        .collect()
}

fn strip_unregistered_emoji_shortcodes(text: &str, allowed: &HashSet<String>) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(text.len());
    let mut index = 0usize;

    while index < chars.len() {
        if chars[index] == ':' {
            let mut end = index + 1;
            while end < chars.len() && is_custom_emoji_shortcode_char(chars[end]) {
                end += 1;
            }
            if end > index + 1 && end < chars.len() && chars[end] == ':' {
                let shortcode = chars[index + 1..end].iter().collect::<String>();
                if allowed.contains(&shortcode) {
                    output.push(':');
                    output.push_str(&shortcode);
                    output.push(':');
                } else if output.ends_with([' ', '\t'])
                    && end + 1 < chars.len()
                    && (chars[end + 1] == ' ' || chars[end + 1] == '\t')
                {
                    index = end + 2;
                    continue;
                }
                index = end + 1;
                continue;
            }
        }
        output.push(chars[index]);
        index += 1;
    }

    output
}

pub(super) fn extract_emoji_shortcodes(text: &str) -> Vec<String> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut shortcodes = Vec::new();
    let mut index = 0usize;

    while index < chars.len() {
        if chars[index] == ':' {
            let mut end = index + 1;
            while end < chars.len() && is_custom_emoji_shortcode_char(chars[end]) {
                end += 1;
            }
            if end > index + 1 && end < chars.len() && chars[end] == ':' {
                shortcodes.push(chars[index + 1..end].iter().collect::<String>());
                index = end + 1;
                continue;
            }
        }
        index += 1;
    }

    shortcodes
}

fn is_custom_emoji_shortcode_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::{
        custom_emojis_used_in_texts, extract_emoji_shortcodes, parse_custom_emojis_json,
        sanitize_emoji_shortcodes, strip_unregistered_emoji_shortcodes,
    };
    use crate::custom_emojis::store::merge_custom_emojis;
    use cfwdon_core::{AppConfig, CustomEmoji};

    fn test_config(emojis: Vec<CustomEmoji>) -> AppConfig {
        let mut config = AppConfig::new("example.com", "cfwdon", "test");
        config.custom_emojis = emojis;
        config
    }

    fn blobaww() -> CustomEmoji {
        CustomEmoji {
            shortcode: "blobaww".to_owned(),
            url: "https://media.example/blobaww.png".to_owned(),
            static_url: "https://media.example/blobaww.png".to_owned(),
            visible_in_picker: true,
            category: None,
        }
    }

    #[test]
    fn merge_custom_emojis_prefers_database_entries() {
        let configured = vec![CustomEmoji {
            shortcode: "blobaww".to_owned(),
            url: "https://media.example/old.png".to_owned(),
            static_url: "https://media.example/old.png".to_owned(),
            visible_in_picker: true,
            category: None,
        }];
        let stored = vec![blobaww()];
        let merged = merge_custom_emojis(&configured, stored);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].url, "https://media.example/blobaww.png");
    }

    #[test]
    fn parse_custom_emojis_json_normalizes_entries() {
        let emojis = parse_custom_emojis_json(
            r#"[
              {"shortcode":" blobaww ","url":" https://media.example/blobaww.png "},
              {"shortcode":"blobaww","url":"https://media.example/duplicate.png"},
              {"shortcode":":invalid:","url":"https://media.example/invalid.png"},
              {"shortcode":"yikes","url":"https://media.example/yikes.png","category":" Reactions "}
            ]"#,
        )
        .unwrap();

        assert_eq!(emojis.len(), 2);
        assert_eq!(emojis[0].shortcode, "blobaww");
        assert_eq!(emojis[1].shortcode, "yikes");
    }

    #[test]
    fn strip_unregistered_emoji_shortcodes_keeps_registered_tokens() {
        let allowed = ["blobaww".to_owned()].into_iter().collect();
        assert_eq!(
            strip_unregistered_emoji_shortcodes("hello :blobaww: :unknown: world", &allowed),
            "hello :blobaww: world"
        );
        assert_eq!(
            strip_unregistered_emoji_shortcodes(":blobcat:", &allowed),
            ""
        );
        assert_eq!(
            strip_unregistered_emoji_shortcodes("12:30", &allowed),
            "12:30"
        );
    }

    #[test]
    fn sanitize_emoji_shortcodes_uses_config_registry() {
        let config = test_config(vec![blobaww()]);
        assert_eq!(
            sanitize_emoji_shortcodes("hi :blobaww: :missing:", &config),
            "hi :blobaww: "
        );
    }

    #[test]
    fn custom_emojis_used_in_text_returns_unique_registered_matches() {
        let config = test_config(vec![blobaww()]);
        let emojis = custom_emojis_used_in_texts([":blobaww: again :blobaww: :missing:"], &config);
        assert_eq!(emojis.len(), 1);
        assert_eq!(emojis[0]["shortcode"], "blobaww");
    }

    #[test]
    fn extract_emoji_shortcodes_finds_colon_tokens() {
        assert_eq!(
            extract_emoji_shortcodes("a :one: b :two:"),
            vec!["one".to_owned(), "two".to_owned()]
        );
    }
}
