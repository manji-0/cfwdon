mod email;
mod record;
mod registration;
mod username;

pub use email::{AccessEmail, AccessEmailError};
pub use record::LocalAccountRecord;
pub use registration::{
    AccessProvisionIntent, AccountKeyMaterial, ComposingAccessProvision, ComposingRegistration,
    RegisteringAccount, RegistrationEvent, RegistrationFieldIssue, RegistrationIntent,
    RegistrationValidationErrors, RegistrationValidationField, registration_field_issue_message,
};
pub use username::{Username, UsernameError};

use crate::error::IdError;
use crate::quote::QuoteApprovalPolicy;
use crate::status::Visibility;
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

    pub fn remote(username: impl Into<String>, domain: impl Into<String>) -> Result<Self, IdError> {
        let domain = domain.into();
        if domain.trim().is_empty() {
            return Err(IdError::Empty);
        }
        Ok(Self {
            username: username.into(),
            domain: Some(domain),
        })
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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ProfileField {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalAccount {
    id: String,
    username: String,
    access_email: String,
    display_name: String,
    bio_html: String,
    bio_text: String,
    fields: Vec<ProfileField>,
    locked: bool,
    bot: bool,
    discoverable: bool,
    default_post_visibility: Visibility,
    default_quote_policy: QuoteApprovalPolicy,
    default_sensitive: bool,
    default_language: Option<String>,
    avatar_object_key: Option<String>,
    avatar_content_type: Option<String>,
    header_object_key: Option<String>,
    header_content_type: Option<String>,
    private_key_jwk: String,
    public_key_pem: String,
    created_at: String,
}

impl LocalAccount {
    pub fn from_record(record: LocalAccountRecord) -> Self {
        let fields = serde_json::from_str(&record.fields_json).unwrap_or_default();
        Self {
            id: record.id,
            username: record.username,
            access_email: record.access_email,
            display_name: record.display_name,
            bio_html: record.bio_html,
            bio_text: record.bio_text,
            fields,
            locked: record.locked != 0,
            bot: record.bot != 0,
            discoverable: record.discoverable != 0,
            default_post_visibility: Visibility::parse(&record.default_post_visibility)
                .unwrap_or(Visibility::Public),
            default_quote_policy: QuoteApprovalPolicy::parse(&record.default_quote_policy)
                .unwrap_or(QuoteApprovalPolicy::Public),
            default_sensitive: record.default_sensitive != 0,
            default_language: record.default_language,
            avatar_object_key: record.avatar_object_key,
            avatar_content_type: record.avatar_content_type,
            header_object_key: record.header_object_key,
            header_content_type: record.header_content_type,
            private_key_jwk: record.private_key_jwk,
            public_key_pem: record.public_key_pem,
            created_at: record.created_at,
        }
    }

    pub fn to_record(&self) -> LocalAccountRecord {
        LocalAccountRecord {
            id: self.id.clone(),
            username: self.username.clone(),
            access_email: self.access_email.clone(),
            display_name: self.display_name.clone(),
            bio_html: self.bio_html.clone(),
            bio_text: self.bio_text.clone(),
            fields_json: serde_json::to_string(&self.fields).unwrap_or_else(|_| "[]".to_owned()),
            locked: i32::from(self.locked),
            bot: i32::from(self.bot),
            discoverable: i32::from(self.discoverable),
            default_post_visibility: self.default_post_visibility.as_str().to_owned(),
            default_quote_policy: self.default_quote_policy.as_str().to_owned(),
            default_sensitive: i32::from(self.default_sensitive),
            default_language: self.default_language.clone(),
            avatar_object_key: self.avatar_object_key.clone(),
            avatar_content_type: self.avatar_content_type.clone(),
            header_object_key: self.header_object_key.clone(),
            header_content_type: self.header_content_type.clone(),
            private_key_jwk: self.private_key_jwk.clone(),
            public_key_pem: self.public_key_pem.clone(),
            created_at: self.created_at.clone(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn access_email(&self) -> &str {
        &self.access_email
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn bio_html(&self) -> &str {
        &self.bio_html
    }

    pub fn bio_text(&self) -> &str {
        &self.bio_text
    }

    pub fn fields(&self) -> &[ProfileField] {
        &self.fields
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn is_bot(&self) -> bool {
        self.bot
    }

    pub fn is_discoverable(&self) -> bool {
        self.discoverable
    }

    pub fn default_visibility(&self) -> Visibility {
        self.default_post_visibility
    }

    pub fn default_quote_policy(&self) -> QuoteApprovalPolicy {
        self.default_quote_policy
    }

    pub fn default_sensitive(&self) -> bool {
        self.default_sensitive
    }

    pub fn default_language(&self) -> Option<&str> {
        self.default_language.as_deref()
    }

    pub fn avatar_object_key(&self) -> Option<&str> {
        self.avatar_object_key.as_deref()
    }

    pub fn avatar_content_type(&self) -> Option<&str> {
        self.avatar_content_type.as_deref()
    }

    pub fn header_object_key(&self) -> Option<&str> {
        self.header_object_key.as_deref()
    }

    pub fn header_content_type(&self) -> Option<&str> {
        self.header_content_type.as_deref()
    }

    pub fn private_key_jwk(&self) -> &str {
        &self.private_key_jwk
    }

    pub fn public_key_pem(&self) -> &str {
        &self.public_key_pem
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }

    pub fn acct(&self) -> &str {
        &self.username
    }

    pub fn resolved_default_quote_policy(&self) -> QuoteApprovalPolicy {
        self.default_quote_policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_account() -> LocalAccount {
        LocalAccount::from_record(LocalAccountRecord {
            id: "acct-1".to_owned(),
            username: "alice".to_owned(),
            access_email: "alice@example.com".to_owned(),
            display_name: "Alice".to_owned(),
            bio_html: "<p>hi</p>".to_owned(),
            bio_text: "hi".to_owned(),
            fields_json: serde_json::to_string(&[ProfileField {
                name: "Site".to_owned(),
                value: "https://example.com".to_owned(),
            }])
            .unwrap(),
            locked: 1,
            bot: 0,
            discoverable: 1,
            default_post_visibility: "unlisted".to_owned(),
            default_quote_policy: "followers".to_owned(),
            default_sensitive: 1,
            default_language: Some("en".to_owned()),
            avatar_object_key: Some("avatars/alice".to_owned()),
            avatar_content_type: Some("image/png".to_owned()),
            header_object_key: None,
            header_content_type: None,
            private_key_jwk: "{\"kty\":\"RSA\"}".to_owned(),
            public_key_pem: "-----BEGIN PUBLIC KEY-----".to_owned(),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
        })
    }

    #[test]
    fn local_account_record_roundtrip_preserves_entity() {
        let account = fixture_account();
        let record = account.to_record();
        let restored = LocalAccount::from_record(record);

        assert_eq!(account, restored);
    }

    #[test]
    fn local_account_from_record_maps_integer_flags() {
        let record = LocalAccountRecord {
            id: "acct-2".to_owned(),
            username: "bob".to_owned(),
            access_email: "bob@example.com".to_owned(),
            display_name: String::new(),
            bio_html: String::new(),
            bio_text: String::new(),
            fields_json: "[]".to_owned(),
            locked: 1,
            bot: 0,
            discoverable: 1,
            default_post_visibility: "public".to_owned(),
            default_quote_policy: "public".to_owned(),
            default_sensitive: 0,
            default_language: None,
            avatar_object_key: None,
            avatar_content_type: None,
            header_object_key: None,
            header_content_type: None,
            private_key_jwk: String::new(),
            public_key_pem: String::new(),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
        };

        let account = LocalAccount::from_record(record);

        assert!(account.is_locked());
        assert!(!account.is_bot());
        assert!(account.is_discoverable());
        assert!(!account.default_sensitive());
    }
}
