use serde::Deserialize;

/// Persistence-shaped local account row loaded from D1 or API adapters.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct LocalAccountRecord {
    pub id: String,
    pub username: String,
    pub access_email: String,
    pub display_name: String,
    pub bio_html: String,
    pub bio_text: String,
    pub fields_json: String,
    pub locked: i32,
    pub bot: i32,
    pub discoverable: i32,
    pub default_post_visibility: String,
    #[serde(default = "default_quote_policy")]
    pub default_quote_policy: String,
    pub default_sensitive: i32,
    pub default_language: Option<String>,
    pub avatar_object_key: Option<String>,
    pub avatar_content_type: Option<String>,
    pub header_object_key: Option<String>,
    pub header_content_type: Option<String>,
    #[serde(default)]
    pub private_key_jwk: String,
    pub public_key_pem: String,
    pub created_at: String,
}

fn default_quote_policy() -> String {
    "public".to_owned()
}

impl LocalAccountRecord {
    /// Builds a minimal account record for tests and worker fixtures.
    pub fn test_fixture(id: impl Into<String>, username: impl Into<String>) -> Self {
        let id = id.into();
        let username = username.into();
        Self {
            id: id.clone(),
            username: username.clone(),
            access_email: format!("{username}@example.com"),
            display_name: username.clone(),
            bio_html: String::new(),
            bio_text: String::new(),
            fields_json: "[]".to_owned(),
            locked: 0,
            bot: 0,
            discoverable: 0,
            default_post_visibility: "public".to_owned(),
            default_quote_policy: "public".to_owned(),
            default_sensitive: 0,
            default_language: Some("en".to_owned()),
            avatar_object_key: None,
            avatar_content_type: None,
            header_object_key: None,
            header_content_type: None,
            private_key_jwk: "{}".to_owned(),
            public_key_pem: "pem".to_owned(),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        }
    }
}
