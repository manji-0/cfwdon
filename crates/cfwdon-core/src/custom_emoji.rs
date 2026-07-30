use serde::{Deserialize, Serialize};

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
