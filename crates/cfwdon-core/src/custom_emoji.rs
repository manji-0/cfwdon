use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Mastodon-compatible custom emoji definition.
///
/// See <https://docs.joinmastodon.org/entities/CustomEmoji/>.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomEmoji {
    pub shortcode: String,
    pub url: String,
    pub static_url: String,
    #[serde(default = "default_visible_in_picker")]
    pub visible_in_picker: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

fn default_visible_in_picker() -> bool {
    true
}

pub fn is_custom_emoji_shortcode(shortcode: &str) -> bool {
    !shortcode.is_empty()
        && shortcode
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

pub fn parse_custom_emojis_json(raw: &str) -> Result<Vec<CustomEmoji>, String> {
    let inputs = serde_json::from_str::<Vec<CustomEmojiInput>>(raw)
        .map_err(|error| format!("invalid CUSTOM_EMOJIS_JSON: {error}"))?;
    Ok(normalize_custom_emojis(inputs))
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
