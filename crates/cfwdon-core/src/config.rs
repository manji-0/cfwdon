use crate::custom_emoji::CustomEmoji;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TimelineAccessLevel {
    #[default]
    Public,
    Authenticated,
    Disabled,
}

impl TimelineAccessLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Authenticated => "authenticated",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    pub instance_domain: String,
    pub instance_name: String,
    pub instance_description: String,
    pub source_url: Option<String>,
    pub instance_languages: Vec<String>,
    pub admin_emails: Vec<String>,
    pub contact_email: Option<String>,
    pub instance_thumbnail_url: Option<String>,
    pub instance_extended_description_html: Option<String>,
    pub instance_extended_description_updated_at: Option<String>,
    pub privacy_policy_html: Option<String>,
    pub privacy_policy_updated_at: Option<String>,
    pub terms_of_service_html: Option<String>,
    pub terms_of_service_effective_date: Option<String>,
    pub announcements_json: Option<String>,
    pub donation_campaign_json: Option<String>,
    pub custom_emojis: Vec<CustomEmoji>,
    pub web_push_vapid_public_key: Option<String>,
    pub web_push_vapid_private_key: Option<String>,
    pub web_push_vapid_subject: Option<String>,
    pub account_private_key_encryption_key: Option<String>,
    pub database_binding: String,
    pub media_binding: String,
    pub remote_dns_cache_binding: String,
    pub stream_hub_binding: String,
    pub inbox_host_binding: String,
    pub media_public_base_url: Option<String>,
    pub timeline_live_feeds_local: TimelineAccessLevel,
    pub timeline_live_feeds_remote: TimelineAccessLevel,
    pub timeline_hashtag_feeds_local: TimelineAccessLevel,
    pub timeline_hashtag_feeds_remote: TimelineAccessLevel,
    pub timeline_trending_link_feeds_local: TimelineAccessLevel,
    pub timeline_trending_link_feeds_remote: TimelineAccessLevel,
    pub auth0_jwt_header: String,
    pub auth0_domain: String,
    pub auth0_client_id: String,
    pub auth0_audience: String,
    pub auth0_email_claim: String,
}

impl AppConfig {
    pub fn new(
        instance_domain: impl Into<String>,
        instance_name: impl Into<String>,
        instance_description: impl Into<String>,
    ) -> Self {
        Self {
            instance_domain: instance_domain.into(),
            instance_name: instance_name.into(),
            instance_description: instance_description.into(),
            source_url: None,
            instance_languages: vec!["en".to_owned()],
            admin_emails: Vec::new(),
            contact_email: None,
            instance_thumbnail_url: None,
            instance_extended_description_html: None,
            instance_extended_description_updated_at: None,
            privacy_policy_html: None,
            privacy_policy_updated_at: None,
            terms_of_service_html: None,
            terms_of_service_effective_date: None,
            announcements_json: None,
            donation_campaign_json: None,
            custom_emojis: Vec::new(),
            web_push_vapid_public_key: None,
            web_push_vapid_private_key: None,
            web_push_vapid_subject: None,
            account_private_key_encryption_key: None,
            database_binding: "DB".to_owned(),
            media_binding: "MEDIA".to_owned(),
            remote_dns_cache_binding: "REMOTE_DNS_CACHE".to_owned(),
            stream_hub_binding: "STREAM_HUB".to_owned(),
            inbox_host_binding: "INBOX_HOST".to_owned(),
            media_public_base_url: None,
            timeline_live_feeds_local: TimelineAccessLevel::Public,
            timeline_live_feeds_remote: TimelineAccessLevel::Public,
            timeline_hashtag_feeds_local: TimelineAccessLevel::Public,
            timeline_hashtag_feeds_remote: TimelineAccessLevel::Public,
            timeline_trending_link_feeds_local: TimelineAccessLevel::Public,
            timeline_trending_link_feeds_remote: TimelineAccessLevel::Public,
            auth0_jwt_header: "Authorization".to_owned(),
            auth0_domain: String::new(),
            auth0_client_id: String::new(),
            auth0_audience: String::new(),
            auth0_email_claim: "email".to_owned(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new(
            "example.com",
            "cfwdon",
            "Cloudflare Workers + D1 + R2 based Mastodon-compatible server",
        )
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildMetadata {
    pub service_name: &'static str,
    pub version: &'static str,
    pub runtime: &'static str,
}

impl BuildMetadata {
    pub const fn new(
        service_name: &'static str,
        version: &'static str,
        runtime: &'static str,
    ) -> Self {
        Self {
            service_name,
            version,
            runtime,
        }
    }
}
