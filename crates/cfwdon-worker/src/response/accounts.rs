use crate::{
    AccountStats, AppConfig, LocalAccount, MastodonAccountResponse, MastodonAccountSource,
    ProfileField, RemoteActorProfile, RemoteActorRow, actor_url, escape_html,
    fetch_remote_activitypub_document, media_object_url, remote_account_rest_id,
};
use std::cell::RefCell;
use std::collections::HashMap;
use url::Url;
use worker::Result;

const REMOTE_ACTOR_COLLECTION_COUNT_CACHE_TTL_MS: f64 = 60.0 * 1000.0;

thread_local! {
    static REMOTE_ACTOR_COLLECTION_COUNT_CACHE: RefCell<HashMap<String, (u64, f64)>> =
        RefCell::new(HashMap::new());
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RemoteActorSocialCounts {
    pub(crate) followers_count: u64,
    pub(crate) following_count: u64,
    pub(crate) statuses_count: u64,
}

pub(crate) async fn load_remote_actor_social_counts_from_document(
    document: &serde_json::Value,
) -> Result<RemoteActorSocialCounts> {
    let followers_count = remote_actor_collection_count(document, "followers")
        .await
        .unwrap_or(0);
    let following_count = remote_actor_collection_count(document, "following")
        .await
        .unwrap_or(0);
    let statuses_count = remote_actor_collection_count(document, "outbox")
        .await
        .unwrap_or(0);
    Ok(RemoteActorSocialCounts {
        followers_count,
        following_count,
        statuses_count,
    })
}

async fn remote_actor_collection_count(
    actor_document: &serde_json::Value,
    field: &str,
) -> Result<u64> {
    let Some(value) = actor_document.get(field) else {
        return Ok(0);
    };
    if let Some(count) = activitypub_collection_count(value) {
        return Ok(count);
    }
    let Some(collection_uri) = activitypub_reference_uri(value) else {
        return Ok(0);
    };
    if let Some(count) = remote_actor_collection_count_cache_hit(&collection_uri) {
        return Ok(count);
    }
    let collection = fetch_remote_activitypub_document(&collection_uri).await?;
    let count = activitypub_collection_count(&collection).unwrap_or(0);
    cache_remote_actor_collection_count(&collection_uri, count);
    Ok(count)
}

fn remote_actor_collection_count_cache_hit(collection_uri: &str) -> Option<u64> {
    let now_ms = js_sys::Date::now();
    REMOTE_ACTOR_COLLECTION_COUNT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.retain(|_, (_, expires_at_ms)| *expires_at_ms > now_ms);
        cache
            .get(collection_uri)
            .filter(|(_, expires_at_ms)| *expires_at_ms > now_ms)
            .map(|(count, _)| *count)
    })
}

fn cache_remote_actor_collection_count(collection_uri: &str, count: u64) {
    let expires_at_ms = js_sys::Date::now() + REMOTE_ACTOR_COLLECTION_COUNT_CACHE_TTL_MS;
    REMOTE_ACTOR_COLLECTION_COUNT_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .insert(collection_uri.to_owned(), (count, expires_at_ms));
    });
}

fn activitypub_reference_uri(value: &serde_json::Value) -> Option<String> {
    if let Some(uri) = value.as_str().map(str::trim).filter(|uri| !uri.is_empty()) {
        return Some(uri.to_owned());
    }
    value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|uri| !uri.is_empty())
        .map(ToOwned::to_owned)
}

fn activitypub_collection_count(collection: &serde_json::Value) -> Option<u64> {
    collection
        .get("totalItems")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            collection
                .get("orderedItems")
                .or_else(|| collection.get("items"))
                .and_then(serde_json::Value::as_array)
                .map(|items| items.len() as u64)
        })
}

pub(crate) fn apply_remote_actor_social_counts(
    account: &mut MastodonAccountResponse,
    counts: RemoteActorSocialCounts,
) {
    account.followers_count = counts.followers_count;
    account.following_count = counts.following_count;
    account.statuses_count = counts.statuses_count;
}

pub(crate) fn mastodon_account_fields(fields: &[ProfileField]) -> Vec<serde_json::Value> {
    fields
        .iter()
        .map(|field| {
            serde_json::json!({
                "name": field.name,
                "value": render_profile_field_value_html(&field.value),
                "verified_at": serde_json::Value::Null,
            })
        })
        .collect()
}

pub(crate) fn render_profile_field_value_html(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if Url::parse(trimmed)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .is_some()
    {
        let escaped = escape_html(trimmed);
        return format!(
            "<a href=\"{escaped}\" rel=\"nofollow noopener noreferrer me\" target=\"_blank\">{escaped}</a>"
        );
    }
    escape_html(trimmed)
}

#[allow(dead_code)]
pub(crate) fn build_preferences_document(account: &LocalAccount) -> serde_json::Value {
    serde_json::json!({
        "posting:default:visibility": account.default_post_visibility,
        "posting:default:sensitive": account.default_sensitive,
        "posting:default:language": account.default_language,
        "posting:default:quote_policy": account.default_quote_policy,
        "posting:default:privacy": account.default_post_visibility,
        "posting:default:media_sensitive": account.default_sensitive,
        "posting:default:content_type": "text/plain",
        "reading:expand:media": "default",
        "reading:expand:spoilers": false,
        "reading:autoplay:gifs": true,
        "reading:display:media": "default",
        "reading:display:expand_media": "default",
        "reading:display:expand_spoilers": false,
        "notifications:follow": true,
        "notifications:favourite": true,
        "notifications:reblog": true,
        "notifications:mention": true,
        "notifications:poll": true,
        "web:theme": "default",
    })
}

fn account_avatar_url(config: &AppConfig, account: &LocalAccount) -> String {
    account
        .avatar_object_key
        .as_deref()
        .map(|object_key| media_object_url(config, object_key))
        .unwrap_or_default()
}

fn account_header_url(config: &AppConfig, account: &LocalAccount) -> String {
    account
        .header_object_key
        .as_deref()
        .map(|object_key| media_object_url(config, object_key))
        .unwrap_or_default()
}

fn account_noindex(indexable: bool) -> Option<bool> {
    (!indexable).then_some(true)
}

impl MastodonAccountResponse {
    pub(crate) fn with_profile_settings(
        mut self,
        indexable: bool,
        hide_collections: Option<bool>,
        show_media: bool,
        show_media_replies: bool,
        show_featured: bool,
    ) -> Self {
        self.indexable = indexable;
        self.noindex = account_noindex(indexable);
        self.hide_collections = hide_collections;
        self.show_media = Some(show_media);
        self.show_media_replies = Some(show_media_replies);
        self.show_featured = Some(show_featured);
        if let Some(source) = self.source.as_mut() {
            source.hide_collections = hide_collections;
            source.indexable = indexable;
        }
        self
    }

    pub(crate) fn from_account(account: &LocalAccount, config: &AppConfig) -> Self {
        Self::from_account_with_stats(account, config, &AccountStats::default())
    }

    pub(crate) fn from_account_with_stats(
        account: &LocalAccount,
        config: &AppConfig,
        stats: &AccountStats,
    ) -> Self {
        let profile_url = actor_url(config, &account.username);

        Self {
            id: account.id.clone(),
            username: account.username.clone(),
            acct: account.acct().to_owned(),
            uri: actor_url(config, &account.username),
            display_name: account.display_name.clone(),
            locked: account.locked,
            bot: account.bot,
            group: false,
            discoverable: account.discoverable,
            indexable: true,
            noindex: None,
            hide_collections: None,
            show_media: Some(true),
            show_media_replies: Some(true),
            show_featured: Some(true),
            last_status_at: stats.last_status_at.clone(),
            created_at: timestamp_to_mastodon_iso8601(&account.created_at),
            note: account.bio_html.clone(),
            url: profile_url,
            avatar: account_avatar_url(config, account),
            avatar_static: account_avatar_url(config, account),
            header: account_header_url(config, account),
            header_static: account_header_url(config, account),
            emojis: Vec::new(),
            fields: mastodon_account_fields(&account.fields),
            roles: Vec::new(),
            followers_count: stats.followers_count,
            following_count: stats.following_count,
            statuses_count: stats.statuses_count,
            source: None,
        }
    }

    pub(crate) fn from_credentials_account(
        account: &LocalAccount,
        config: &AppConfig,
        stats: &AccountStats,
    ) -> Self {
        let mut value = Self::from_account_with_stats(account, config, stats);
        value.source = Some(MastodonAccountSource {
            note: account.bio_text.clone(),
            fields: mastodon_account_fields(&account.fields),
            attribution_domains: Vec::new(),
            privacy: account.default_post_visibility.clone(),
            sensitive: account.default_sensitive,
            language: account.default_language.clone().unwrap_or_default(),
            follow_requests_count: 0,
            hide_collections: None,
            discoverable: Some(account.discoverable),
            indexable: true,
            quote_policy: account.default_quote_policy.clone(),
        });
        value
    }

    pub(crate) fn from_remote_actor(actor: &RemoteActorRow) -> Self {
        let profile_url = actor
            .profile_url
            .clone()
            .unwrap_or_else(|| actor.actor_uri.clone());
        let avatar_url = actor.avatar_url.clone().unwrap_or_default();
        let header_url = actor.header_url.clone().unwrap_or_default();
        let created_at = if actor.created_at.trim().is_empty() {
            "1970-01-01T00:00:00.000Z".to_owned()
        } else {
            timestamp_to_mastodon_iso8601(&actor.created_at)
        };

        Self {
            id: remote_account_rest_id(&actor.actor_uri),
            username: actor.username.clone(),
            acct: format!("{}@{}", actor.username, actor.domain),
            uri: actor.actor_uri.clone(),
            display_name: actor.display_name.clone(),
            locked: actor.locked,
            bot: actor.bot,
            group: false,
            discoverable: actor.discoverable,
            indexable: actor.indexable,
            noindex: account_noindex(actor.indexable),
            hide_collections: None,
            show_media: None,
            show_media_replies: None,
            show_featured: None,
            last_status_at: None,
            created_at,
            note: actor.summary_html.clone(),
            url: profile_url,
            avatar: avatar_url.clone(),
            avatar_static: avatar_url,
            header: header_url.clone(),
            header_static: header_url,
            emojis: Vec::new(),
            fields: Vec::new(),
            roles: Vec::new(),
            followers_count: 0,
            following_count: 0,
            statuses_count: 0,
            source: None,
        }
    }

    pub(crate) fn from_remote_actor_profile(actor: &RemoteActorProfile) -> Self {
        let profile_url = actor
            .profile_url
            .clone()
            .unwrap_or_else(|| actor.actor_uri.clone());
        let avatar_url = actor.avatar_url.clone().unwrap_or_default();
        let header_url = actor.header_url.clone().unwrap_or_default();

        Self {
            id: remote_account_rest_id(&actor.actor_uri),
            username: actor.username.clone(),
            acct: format!("{}@{}", actor.username, actor.domain),
            uri: actor.actor_uri.clone(),
            display_name: actor.display_name.clone(),
            locked: actor.locked,
            bot: actor.bot,
            group: false,
            discoverable: actor.discoverable,
            indexable: actor.indexable,
            noindex: account_noindex(actor.indexable),
            hide_collections: None,
            show_media: None,
            show_media_replies: None,
            show_featured: None,
            last_status_at: None,
            created_at: "1970-01-01T00:00:00.000Z".to_owned(),
            note: actor.summary_html.clone(),
            url: profile_url,
            avatar: avatar_url.clone(),
            avatar_static: avatar_url,
            header: header_url.clone(),
            header_static: header_url,
            emojis: Vec::new(),
            fields: Vec::new(),
            roles: Vec::new(),
            followers_count: 0,
            following_count: 0,
            statuses_count: 0,
            source: None,
        }
    }
}

pub(crate) fn timestamp_to_mastodon_iso8601(value: &str) -> String {
    let value = value.trim();
    if value.contains('T') {
        return value.to_owned();
    }
    if value.len() == "YYYY-MM-DD HH:MM:SS".len() {
        return format!("{}T{}.000Z", &value[..10], &value[11..]);
    }
    value.to_owned()
}
