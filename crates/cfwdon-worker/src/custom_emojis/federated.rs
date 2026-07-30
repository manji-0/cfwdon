use super::{custom_emoji_to_json, extract_emoji_shortcodes};
use crate::sanitize_remote_http_url;
use cfwdon_core::{AppConfig, CustomEmoji, is_custom_emoji_shortcode};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use worker::{D1Database, Result, d1::D1Type};

pub(crate) type FederatedEmojiMap = HashMap<String, CustomEmoji>;

#[derive(Debug, Default)]
pub(crate) struct RemoteStatusFederatedEmojisPreload {
    by_status_id: HashMap<String, FederatedEmojiMap>,
}

impl RemoteStatusFederatedEmojisPreload {
    pub(crate) fn get(&self, status_id: &str) -> Option<&FederatedEmojiMap> {
        self.by_status_id.get(status_id)
    }
}

pub(crate) async fn preload_remote_status_federated_emojis(
    db: &D1Database,
    status_ids: &[String],
) -> Result<RemoteStatusFederatedEmojisPreload> {
    if status_ids.is_empty() {
        return Ok(RemoteStatusFederatedEmojisPreload::default());
    }

    let placeholders = (1..=status_ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let query = format!(
        "SELECT id, raw_object_json
         FROM remote_statuses
         WHERE id IN ({placeholders})"
    );
    let bindings = status_ids
        .iter()
        .map(|status_id| D1Type::Text(status_id.as_str()))
        .collect::<Vec<_>>();
    let rows = db
        .prepare(&query)
        .bind_refs(bindings.iter())?
        .all()
        .await?
        .results::<RemoteStatusRawObjectRow>()?;

    let mut by_status_id = HashMap::with_capacity(rows.len());
    for row in rows {
        if let Ok(object) = serde_json::from_str::<serde_json::Value>(&row.raw_object_json) {
            by_status_id.insert(
                row.id,
                extract_federated_emojis_from_activitypub_object(&object),
            );
        }
    }

    Ok(RemoteStatusFederatedEmojisPreload { by_status_id })
}

pub(crate) fn extract_federated_emojis_from_activitypub_object(
    object: &serde_json::Value,
) -> FederatedEmojiMap {
    let mut emojis = FederatedEmojiMap::new();
    for tag in activitypub_tag_entries(object) {
        let Some(emoji) = parse_activitypub_emoji_tag(tag) else {
            continue;
        };
        emojis.insert(emoji.shortcode.clone(), emoji);
    }
    emojis
}

pub(crate) fn resolve_status_emojis(
    federated: Option<&FederatedEmojiMap>,
    texts: &[&str],
    config: &AppConfig,
) -> Vec<serde_json::Value> {
    let local_registry = local_emoji_registry(config);
    let mut seen = HashSet::new();
    let mut emojis = Vec::new();

    for text in texts {
        for shortcode in extract_emoji_shortcodes(text) {
            if !seen.insert(shortcode.clone()) {
                continue;
            }
            if let Some(emoji) = federated.and_then(|map| map.get(&shortcode)) {
                emojis.push(custom_emoji_to_json(emoji));
            } else if let Some(emoji) = local_registry.get(&shortcode) {
                emojis.push(custom_emoji_to_json(emoji));
            }
        }
    }

    emojis
}

pub(crate) fn resolve_account_emojis_from_document(
    document: &serde_json::Value,
    display_name: &str,
    note: &str,
    fields: &[(String, String)],
) -> Vec<serde_json::Value> {
    let federated = extract_federated_emojis_from_activitypub_object(document);
    if federated.is_empty() {
        return Vec::new();
    }

    let mut texts = vec![display_name, note];
    let mut field_texts = Vec::new();
    for (name, value) in fields {
        field_texts.push(name.as_str());
        field_texts.push(value.as_str());
    }
    texts.extend(field_texts);

    let mut seen = HashSet::new();
    let mut emojis = Vec::new();
    for text in texts {
        for shortcode in extract_emoji_shortcodes(text) {
            if seen.insert(shortcode.clone())
                && let Some(emoji) = federated.get(&shortcode)
            {
                emojis.push(custom_emoji_to_json(emoji));
            }
        }
    }
    emojis
}

fn local_emoji_registry(config: &AppConfig) -> HashMap<String, CustomEmoji> {
    config
        .custom_emojis
        .iter()
        .map(|emoji| (emoji.shortcode.clone(), emoji.clone()))
        .collect()
}

fn activitypub_tag_entries<'a>(
    object: &'a serde_json::Value,
) -> Box<dyn Iterator<Item = &'a serde_json::Value> + 'a> {
    match object.get("tag") {
        Some(serde_json::Value::Array(tags)) => Box::new(tags.iter()),
        Some(tag @ serde_json::Value::Object(_)) => Box::new(std::iter::once(tag)),
        _ => Box::new(std::iter::empty()),
    }
}

fn parse_activitypub_emoji_tag(tag: &serde_json::Value) -> Option<CustomEmoji> {
    let tag_type = tag
        .get("type")
        .or_else(|| tag.get("@type"))
        .and_then(serde_json::Value::as_str)?;
    if !tag_type.eq_ignore_ascii_case("emoji") {
        return None;
    }

    let shortcode = normalize_activitypub_shortcode(tag.get("name").and_then(|v| v.as_str())?)?;
    let url = emoji_icon_url(tag.get("icon")?)?;
    let static_url = tag
        .get("icon")
        .and_then(emoji_static_icon_url)
        .unwrap_or_else(|| url.clone());

    Some(CustomEmoji {
        shortcode,
        url,
        static_url,
        visible_in_picker: false,
        category: None,
    })
}

fn normalize_activitypub_shortcode(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let shortcode = trimmed
        .strip_prefix(':')
        .and_then(|value| value.strip_suffix(':'))
        .unwrap_or(trimmed);
    if shortcode.is_empty() || !is_custom_emoji_shortcode(shortcode) {
        return None;
    }
    Some(shortcode.to_owned())
}

fn emoji_icon_url(icon: &serde_json::Value) -> Option<String> {
    let raw = icon
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            icon.get("url")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })?;
    sanitize_remote_http_url(Some(raw.as_str()))
}

fn emoji_static_icon_url(icon: &serde_json::Value) -> Option<String> {
    let raw = icon
        .get("staticUrl")
        .or_else(|| icon.get("static_url"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    sanitize_remote_http_url(Some(raw))
}

#[derive(Debug, Deserialize)]
struct RemoteStatusRawObjectRow {
    id: String,
    raw_object_json: String,
}

#[cfg(test)]
mod tests {
    use super::{
        extract_federated_emojis_from_activitypub_object, resolve_account_emojis_from_document,
        resolve_status_emojis,
    };
    use cfwdon_core::{AppConfig, CustomEmoji};

    #[test]
    fn extracts_federated_emojis_from_note_tags() {
        let object = serde_json::json!({
            "type": "Note",
            "tag": [{
                "type": "Emoji",
                "name": ":blobcat:",
                "icon": {
                    "type": "Image",
                    "mediaType": "image/png",
                    "url": "https://remote.example/emoji/blobcat.png"
                }
            }]
        });
        let emojis = extract_federated_emojis_from_activitypub_object(&object);
        assert_eq!(emojis.len(), 1);
        assert_eq!(
            emojis.get("blobcat").unwrap().url,
            "https://remote.example/emoji/blobcat.png"
        );
    }

    #[test]
    fn resolve_status_emojis_prefers_federated_urls() {
        let config = AppConfig {
            custom_emojis: vec![CustomEmoji {
                shortcode: "blobcat".to_owned(),
                url: "https://local.example/emoji/blobcat.gif".to_owned(),
                static_url: "https://local.example/emoji/blobcat.gif".to_owned(),
                visible_in_picker: true,
                category: None,
            }],
            ..AppConfig::default()
        };
        let federated = extract_federated_emojis_from_activitypub_object(&serde_json::json!({
            "tag": [{
                "type": "Emoji",
                "name": ":blobcat:",
                "icon": { "url": "https://remote.example/emoji/blobcat.png" }
            }]
        }));
        let emojis = resolve_status_emojis(Some(&federated), &["hello :blobcat:"], &config);
        assert_eq!(emojis.len(), 1);
        assert_eq!(
            emojis[0].get("url").and_then(|value| value.as_str()),
            Some("https://remote.example/emoji/blobcat.png")
        );
    }

    #[test]
    fn resolve_account_emojis_uses_profile_text() {
        let document = serde_json::json!({
            "tag": [{
                "type": "Emoji",
                "name": ":party:",
                "icon": { "url": "https://remote.example/emoji/party.gif" }
            }]
        });
        let emojis = resolve_account_emojis_from_document(&document, "I like :party:", "", &[]);
        assert_eq!(emojis.len(), 1);
        assert_eq!(
            emojis[0].get("shortcode").and_then(|value| value.as_str()),
            Some("party")
        );
    }

    #[test]
    fn rejects_non_http_federated_emoji_urls() {
        let object = serde_json::json!({
            "tag": [{
                "type": "Emoji",
                "name": ":evil:",
                "icon": { "url": "javascript:alert(1)" }
            }]
        });
        assert!(extract_federated_emojis_from_activitypub_object(&object).is_empty());
    }
}
