use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountHandle {
    pub username: String,
    pub domain: Option<String>,
}

impl AccountHandle {
    pub fn local(username: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            domain: None,
        }
    }

    pub fn is_local_to(&self, local_domain: &str) -> bool {
        let local_domain = local_domain
            .trim()
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .split('/')
            .next()
            .unwrap_or(local_domain)
            .to_ascii_lowercase();

        match &self.domain {
            Some(domain) => domain.eq_ignore_ascii_case(&local_domain),
            None => true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileField {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalAccount {
    pub id: String,
    pub username: String,
    pub access_email: String,
    pub display_name: String,
    pub bio_html: String,
    pub bio_text: String,
    pub fields: Vec<ProfileField>,
    pub locked: bool,
    pub bot: bool,
    pub discoverable: bool,
    pub default_post_visibility: String,
    pub default_sensitive: bool,
    pub default_language: Option<String>,
    pub avatar_object_key: Option<String>,
    pub avatar_content_type: Option<String>,
    pub header_object_key: Option<String>,
    pub header_content_type: Option<String>,
    pub private_key_jwk: String,
    pub public_key_pem: String,
    pub created_at: String,
}

impl LocalAccount {
    pub fn acct(&self) -> &str {
        &self.username
    }
}
