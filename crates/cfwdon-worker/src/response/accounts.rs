use crate::{
    AccountStats, AppConfig, D1Database, FetchedRemoteActorProfile, LocalAccount,
    MastodonAccountResponse, MastodonAccountRole, MastodonAccountSource, ProfileField,
    RemoteActorProfile, RemoteActorRow, actor_url, custom_emojis_used_in_texts, escape_html,
    fetch_remote_activitypub_document, fetch_signed_activitypub_document,
    load_remote_actor_status_summary, log_json_event, media_object_url,
    parse_remote_actor_profile_document, remote_account_rest_id,
    resolve_account_emojis_from_document, update_remote_actor_social_counts,
    validate_remote_actor_profile_urls,
};
use std::cell::RefCell;
use std::collections::HashMap;
use url::Url;
use worker::{Error, Result};

const REMOTE_ACTOR_COLLECTION_COUNT_CACHE_TTL_MS: f64 = 60.0 * 1000.0;
/// Persist remote F/F/outbox totals for this long before re-fetching collections.
pub(crate) const REMOTE_ACTOR_SOCIAL_COUNTS_TTL_MS: f64 = 60.0 * 60.0 * 1000.0;

thread_local! {
    static REMOTE_ACTOR_COLLECTION_COUNT_CACHE: RefCell<HashMap<String, (u64, f64)>> =
        RefCell::new(HashMap::new());
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RemoteActorSocialCounts {
    pub(crate) followers_count: Option<u64>,
    pub(crate) following_count: Option<u64>,
    pub(crate) statuses_count: Option<u64>,
}

impl RemoteActorSocialCounts {
    pub(crate) fn has_any(self) -> bool {
        self.followers_count.is_some()
            || self.following_count.is_some()
            || self.statuses_count.is_some()
    }
}

/// Controls whether ActivityPub GETs may use HTTP signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteFetchAuthMode {
    /// Public documents only — skip signed GET (avoids slow signed failures / WAF paths).
    Public,
    /// Prefer signed GET when a local signer is available, then fall back to unsigned.
    Authorized,
}

pub(crate) struct RemoteCollectionFetchContext<'a> {
    pub(crate) config: &'a AppConfig,
    pub(crate) db: &'a D1Database,
    pub(crate) signer: Option<&'a LocalAccount>,
    pub(crate) auth_mode: RemoteFetchAuthMode,
}

impl<'a> RemoteCollectionFetchContext<'a> {
    pub(crate) fn public(
        config: &'a AppConfig,
        db: &'a D1Database,
        signer: Option<&'a LocalAccount>,
    ) -> Self {
        Self {
            config,
            db,
            signer,
            auth_mode: RemoteFetchAuthMode::Public,
        }
    }

    pub(crate) fn authorized(
        config: &'a AppConfig,
        db: &'a D1Database,
        signer: Option<&'a LocalAccount>,
    ) -> Self {
        Self {
            config,
            db,
            signer,
            auth_mode: RemoteFetchAuthMode::Authorized,
        }
    }
}

pub(crate) async fn load_remote_actor_social_counts_from_document_with_context(
    document: &serde_json::Value,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
) -> Result<RemoteActorSocialCounts> {
    Ok(RemoteActorSocialCounts {
        followers_count: remote_actor_collection_count(document, "followers", fetch_context).await,
        following_count: remote_actor_collection_count(document, "following", fetch_context).await,
        statuses_count: remote_actor_collection_count(document, "outbox", fetch_context).await,
    })
}

pub(crate) async fn persist_and_apply_remote_actor_social_counts(
    db: &D1Database,
    actor_uri: &str,
    account: &mut MastodonAccountResponse,
    document: &serde_json::Value,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
) -> Result<()> {
    let counts =
        load_remote_actor_social_counts_from_document_with_context(document, fetch_context).await?;
    persist_remote_actor_social_counts(db, actor_uri, counts).await?;
    apply_remote_actor_social_counts(account, counts);
    Ok(())
}

pub(crate) async fn persist_remote_actor_social_counts(
    db: &D1Database,
    actor_uri: &str,
    counts: RemoteActorSocialCounts,
) -> Result<bool> {
    if !counts.has_any() {
        return Ok(false);
    }
    update_remote_actor_social_counts(db, actor_uri, &counts).await?;
    Ok(true)
}

/// Refresh collection totals when missing or older than [`REMOTE_ACTOR_SOCIAL_COUNTS_TTL_MS`].
pub(crate) async fn enrich_remote_account_social_counts(
    db: &D1Database,
    actor_uri: &str,
    social_counts_updated_at: Option<&str>,
    account: &mut MastodonAccountResponse,
    document: &serde_json::Value,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
) -> Result<()> {
    if remote_actor_social_counts_are_fresh(social_counts_updated_at) {
        return Ok(());
    }
    persist_and_apply_remote_actor_social_counts(db, actor_uri, account, document, fetch_context)
        .await
}

pub(crate) fn remote_actor_social_counts_are_fresh(updated_at: Option<&str>) -> bool {
    let Some(updated_at) = updated_at else {
        return false;
    };
    let Some(updated_at_ms) = parse_sqlite_utc_timestamp_ms(updated_at) else {
        return false;
    };
    let age_ms = js_sys::Date::now() - updated_at_ms;
    (0.0..REMOTE_ACTOR_SOCIAL_COUNTS_TTL_MS).contains(&age_ms)
}

fn parse_sqlite_utc_timestamp_ms(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let iso = if value.contains('T') {
        if value.ends_with('Z')
            || value.contains('+')
            || value.rfind('-').is_some_and(|idx| idx > 10)
        {
            value.to_owned()
        } else {
            format!("{value}Z")
        }
    } else if value.len() >= "YYYY-MM-DD HH:MM:SS".len() {
        format!("{}T{}Z", &value[..10], &value[11..19])
    } else {
        return None;
    };
    let parsed = js_sys::Date::new(&iso.into()).get_time();
    (!parsed.is_nan()).then_some(parsed)
}

/// Prefer AP/DB counts; raise response `statuses_count` from local inventory without
/// persisting that ratchet (avoids locking embeds to an inflated local total).
pub(crate) async fn reconcile_remote_account_status_summary(
    db: &D1Database,
    actor_uri: &str,
    account: &mut MastodonAccountResponse,
) -> Result<()> {
    let summary = load_remote_actor_status_summary(db, actor_uri).await?;
    account.last_status_at = summary.last_status_at.clone();
    if summary.statuses_count > account.statuses_count {
        account.statuses_count = summary.statuses_count;
    }
    Ok(())
}

pub(crate) async fn enrich_remote_account_response(
    db: &D1Database,
    actor_uri: &str,
    social_counts_updated_at: Option<&str>,
    account: &mut MastodonAccountResponse,
    document: &serde_json::Value,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
) -> Result<()> {
    if let Err(error) = enrich_remote_account_social_counts(
        db,
        actor_uri,
        social_counts_updated_at,
        account,
        document,
        fetch_context,
    )
    .await
    {
        log_remote_account_enrichment_failure(actor_uri, "social_counts", &error);
    }
    if let Err(error) = reconcile_remote_account_status_summary(db, actor_uri, account).await {
        log_remote_account_enrichment_failure(actor_uri, "status_summary", &error);
    }
    account.emojis = resolve_account_emojis_from_document(
        document,
        &account.display_name,
        &account.note,
        &account
            .fields
            .iter()
            .filter_map(|field| {
                let name = field.get("name")?.as_str()?;
                let value = field.get("value")?.as_str()?;
                Some((name.to_owned(), value.to_owned()))
            })
            .collect::<Vec<_>>(),
    );
    Ok(())
}

fn log_remote_account_enrichment_failure(actor_uri: &str, stage: &str, error: &Error) {
    log_json_event(serde_json::json!({
        "event": "remote_account_enrichment_failed",
        "actor_uri": actor_uri,
        "stage": stage,
        "error": error.to_string(),
    }));
}

pub(crate) fn signed_fetch_allows_unsigned_fallback(error: &Error) -> bool {
    // Public ActivityPub GETs should fall back to an unsigned fetch whenever the
    // signed attempt fails. Auth-rejection (401/403) is the common case, but some
    // remotes (and Cloudflare WAF in front of them) return other status codes or
    // non-JSON challenge bodies that still leave the unsigned public document
    // reachable.
    let message = error.to_string();
    message.contains("private signing key is missing")
        || message.contains("HTTP ")
        || message.contains("error decoding response body")
        || message.contains("expected value")
        || message.contains("invalid type")
        || message.contains("EOF while parsing")
}

pub(crate) async fn fetch_activitypub_document_with_context(
    url: &str,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
) -> Result<serde_json::Value> {
    if let Some(context) = fetch_context
        && context.auth_mode == RemoteFetchAuthMode::Authorized
        && let Some(signer) = context.signer
    {
        match fetch_signed_activitypub_document(context.config, context.db, signer, url).await {
            Ok(document) => return Ok(document),
            Err(error) if signed_fetch_allows_unsigned_fallback(&error) => {
                log_json_event(serde_json::json!({
                    "event": "signed_remote_fetch_fallback",
                    "url": url,
                    "error": error.to_string(),
                }));
            }
            Err(error) => return Err(error),
        }
    }
    fetch_remote_activitypub_document(url).await
}

pub(crate) async fn fetch_remote_actor_profile_with_context(
    actor_uri: &str,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
) -> Result<FetchedRemoteActorProfile> {
    let document = fetch_activitypub_document_with_context(actor_uri, fetch_context).await?;
    let profile = parse_remote_actor_profile_document(&document, actor_uri)?;
    validate_remote_actor_profile_urls(&profile).await?;
    Ok(FetchedRemoteActorProfile { document, profile })
}

async fn remote_actor_collection_count(
    actor_document: &serde_json::Value,
    field: &str,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
) -> Option<u64> {
    let value = actor_document.get(field)?;
    if let Some(count) = activitypub_collection_total_items(value) {
        return Some(count);
    }
    if value.get("first").is_some() {
        return resolve_collection_count_via_first(value, fetch_context).await;
    }
    if let Some(count) = activitypub_collection_items_len(value) {
        return Some(count);
    }
    let collection_uri = activitypub_reference_uri(value)?;
    if let Some(count) = remote_actor_collection_count_cache_hit(&collection_uri) {
        return Some(count);
    }
    let collection = fetch_activitypub_document_with_context(&collection_uri, fetch_context)
        .await
        .ok()?;
    let count = resolve_fetched_collection_count(&collection, fetch_context).await?;
    cache_remote_actor_collection_count(&collection_uri, count);
    Some(count)
}

async fn resolve_fetched_collection_count(
    collection: &serde_json::Value,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
) -> Option<u64> {
    if let Some(count) = activitypub_collection_total_items(collection) {
        return Some(count);
    }
    if collection.get("first").is_some() {
        return resolve_collection_count_via_first(collection, fetch_context).await;
    }
    // Fully embedded collection with no pagination link: items length is the total.
    activitypub_collection_items_len(collection)
}

async fn resolve_collection_count_via_first(
    collection: &serde_json::Value,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
) -> Option<u64> {
    let first = collection.get("first")?;
    if let Some(count) = activitypub_collection_total_items(first) {
        return Some(count);
    }
    // Prefer resolving by URI (string or object id) and only accept totalItems.
    // Never treat a page's orderedItems length as the collection total.
    let first_uri = activitypub_reference_uri(first)?;
    if let Some(count) = remote_actor_collection_count_cache_hit(&first_uri) {
        return Some(count);
    }
    let page = fetch_activitypub_document_with_context(&first_uri, fetch_context)
        .await
        .ok()?;
    let count = activitypub_collection_total_items(&page)?;
    cache_remote_actor_collection_count(&first_uri, count);
    Some(count)
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

fn activitypub_total_items_value(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| {
            value
                .as_i64()
                .filter(|number| *number >= 0)
                .map(|number| number as u64)
        })
        .or_else(|| {
            value
                .as_f64()
                .filter(|number| *number >= 0.0 && number.fract() == 0.0)
                .map(|number| number as u64)
        })
        .or_else(|| value.as_str().and_then(|raw| raw.trim().parse().ok()))
}

fn activitypub_collection_total_items(collection: &serde_json::Value) -> Option<u64> {
    collection
        .get("totalItems")
        .and_then(activitypub_total_items_value)
}

fn activitypub_collection_items_len(collection: &serde_json::Value) -> Option<u64> {
    collection
        .get("orderedItems")
        .or_else(|| collection.get("items"))
        .and_then(serde_json::Value::as_array)
        .map(|items| items.len() as u64)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn activitypub_collection_count(collection: &serde_json::Value) -> Option<u64> {
    activitypub_collection_total_items(collection).or_else(|| {
        if collection.get("first").is_some() {
            None
        } else {
            activitypub_collection_items_len(collection)
        }
    })
}

pub(crate) fn apply_remote_actor_social_counts(
    account: &mut MastodonAccountResponse,
    counts: RemoteActorSocialCounts,
) {
    if let Some(followers_count) = counts.followers_count {
        account.followers_count = followers_count;
    }
    if let Some(following_count) = counts.following_count {
        account.following_count = following_count;
    }
    if let Some(statuses_count) = counts.statuses_count {
        account.statuses_count = statuses_count;
    }
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

pub(crate) fn mastodon_account_source_fields(fields: &[ProfileField]) -> Vec<serde_json::Value> {
    fields
        .iter()
        .map(|field| {
            serde_json::json!({
                "name": field.name,
                "value": field.value,
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
        "posting:default:visibility": account.default_visibility().as_str(),
        "posting:default:sensitive": account.default_sensitive(),
        "posting:default:language": account.default_language(),
        "posting:default:quote_policy": account.default_quote_policy().as_str(),
        "reading:expand:media": "default",
        "reading:expand:spoilers": false,
    })
}

fn account_emojis(account: &LocalAccount, config: &AppConfig) -> Vec<serde_json::Value> {
    let mut texts = vec![account.display_name(), account.bio_text()];
    for field in account.fields() {
        texts.push(field.name.as_str());
        texts.push(field.value.as_str());
    }
    custom_emojis_used_in_texts(texts, config)
}

fn account_avatar_url(config: &AppConfig, account: &LocalAccount) -> String {
    account
        .avatar_object_key()
        .map(|object_key| media_object_url(config, object_key))
        .unwrap_or_default()
}

fn account_header_url(config: &AppConfig, account: &LocalAccount) -> String {
    account
        .header_object_key()
        .map(|object_key| media_object_url(config, object_key))
        .unwrap_or_default()
}

fn account_noindex(indexable: bool) -> Option<bool> {
    (!indexable).then_some(true)
}

fn local_feature_approval() -> serde_json::Value {
    serde_json::json!({
        "automatic": ["public"],
        "manual": [],
        "current_user": "automatic",
    })
}

fn remote_feature_approval() -> serde_json::Value {
    serde_json::json!({
        "automatic": [],
        "manual": [],
        "current_user": "missing",
    })
}

impl MastodonAccountResponse {
    pub(crate) fn with_profile_settings(
        mut self,
        indexable: bool,
        hide_collections: Option<bool>,
        show_media: bool,
        show_media_replies: bool,
        show_featured: bool,
        avatar_description: String,
        header_description: String,
    ) -> Self {
        self.indexable = indexable;
        self.noindex = account_noindex(indexable);
        self.hide_collections = hide_collections;
        self.show_media = Some(show_media);
        self.show_media_replies = Some(show_media_replies);
        self.show_featured = Some(show_featured);
        self.avatar_description = avatar_description;
        self.header_description = header_description;
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
        let profile_url = actor_url(config, account.username());

        Self {
            id: account.id().to_owned(),
            username: account.username().to_owned(),
            acct: account.acct().to_owned(),
            uri: actor_url(config, account.username()),
            display_name: account.display_name().to_owned(),
            locked: account.is_locked(),
            bot: account.is_bot(),
            group: false,
            discoverable: account.is_discoverable(),
            indexable: true,
            noindex: None,
            hide_collections: None,
            show_media: Some(true),
            show_media_replies: Some(true),
            show_featured: Some(true),
            last_status_at: stats.last_status_at.clone(),
            created_at: timestamp_to_mastodon_account_created_at(account.created_at()),
            note: account.bio_html().to_owned(),
            url: profile_url,
            avatar: account_avatar_url(config, account),
            avatar_static: account_avatar_url(config, account),
            avatar_description: String::new(),
            header: account_header_url(config, account),
            header_static: account_header_url(config, account),
            header_description: String::new(),
            emojis: account_emojis(account, config),
            fields: mastodon_account_fields(account.fields()),
            roles: Some(Vec::new()),
            feature_approval: local_feature_approval(),
            followers_count: stats.followers_count,
            following_count: stats.following_count,
            statuses_count: stats.statuses_count,
            source: None,
            role: None,
        }
    }

    pub(crate) fn from_credentials_account(
        account: &LocalAccount,
        config: &AppConfig,
        stats: &AccountStats,
    ) -> Self {
        let mut value = Self::from_account_with_stats(account, config, stats);
        value.source = Some(MastodonAccountSource {
            note: account.bio_text().to_owned(),
            fields: mastodon_account_source_fields(account.fields()),
            attribution_domains: Vec::new(),
            privacy: account.default_visibility().as_str().to_owned(),
            sensitive: account.default_sensitive(),
            language: account.default_language().unwrap_or("").to_owned(),
            follow_requests_count: 0,
            hide_collections: None,
            discoverable: Some(account.is_discoverable()),
            indexable: true,
            quote_policy: account.default_quote_policy().as_str().to_owned(),
        });
        value.role = Some(MastodonAccountRole {
            id: "-99".to_owned(),
            name: String::new(),
            permissions: "0".to_owned(),
            color: String::new(),
            highlighted: false,
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
            timestamp_to_mastodon_account_created_at(&actor.created_at)
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
            note: crate::sanitize_remote_status_html(&actor.summary_html),
            url: profile_url,
            avatar: avatar_url.clone(),
            avatar_static: avatar_url,
            avatar_description: String::new(),
            header: header_url.clone(),
            header_static: header_url,
            header_description: String::new(),
            emojis: Vec::new(),
            fields: Vec::new(),
            roles: None,
            feature_approval: remote_feature_approval(),
            followers_count: actor.followers_count,
            following_count: actor.following_count,
            statuses_count: actor.statuses_count,
            source: None,
            role: None,
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
            note: crate::sanitize_remote_status_html(&actor.summary_html),
            url: profile_url,
            avatar: avatar_url.clone(),
            avatar_static: avatar_url,
            avatar_description: String::new(),
            header: header_url.clone(),
            header_static: header_url,
            header_description: String::new(),
            emojis: Vec::new(),
            fields: Vec::new(),
            roles: None,
            feature_approval: remote_feature_approval(),
            followers_count: 0,
            following_count: 0,
            statuses_count: 0,
            source: None,
            role: None,
        }
    }
}

/// Normalize persisted timestamps into ISO-8601 for Mastodon API exposure.
///
/// Already-ISO values (containing `T`) are preserved as-is. SQLite
/// `YYYY-MM-DD HH:MM:SS` values are treated as UTC and rewritten with
/// Mastodon-style millisecond precision.
pub(crate) fn timestamp_to_mastodon_iso8601(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return String::new();
    }
    if value.contains('T') {
        return value.to_owned();
    }
    let normalized = crate::activitypub_datetime_string(value);
    if normalized.ends_with('Z') && !normalized.contains('.') {
        format!("{}.000Z", &normalized[..normalized.len() - 1])
    } else {
        normalized
    }
}

pub(crate) fn timestamp_to_mastodon_iso8601_opt(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(timestamp_to_mastodon_iso8601)
}

pub(crate) fn timestamp_to_mastodon_account_created_at(value: &str) -> String {
    let normalized = timestamp_to_mastodon_iso8601(value);
    let date = normalized.split(['T', ' ']).next().unwrap_or("1970-01-01");
    if date.len() >= 10 {
        format!("{}T00:00:00.000Z", &date[..10])
    } else {
        "1970-01-01T00:00:00.000Z".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_to_mastodon_iso8601_normalizes_sqlite_timestamp() {
        assert_eq!(
            timestamp_to_mastodon_iso8601("2026-05-09 13:40:48"),
            "2026-05-09T13:40:48.000Z"
        );
        assert_eq!(
            timestamp_to_mastodon_iso8601("2026-05-09T13:40:48.000Z"),
            "2026-05-09T13:40:48.000Z"
        );
    }

    #[test]
    fn activitypub_collection_count_reads_numeric_and_string_total_items() {
        assert_eq!(
            activitypub_collection_count(&serde_json::json!({ "totalItems": 12 })),
            Some(12)
        );
        assert_eq!(
            activitypub_collection_count(&serde_json::json!({ "totalItems": "34" })),
            Some(34)
        );
    }

    #[test]
    fn activitypub_collection_count_prefers_total_items_over_page_length() {
        assert_eq!(
            activitypub_collection_count(&serde_json::json!({
                "totalItems": 100,
                "orderedItems": ["a", "b"]
            })),
            Some(100)
        );
    }

    #[test]
    fn activitypub_collection_count_skips_items_len_when_first_present_without_total() {
        assert_eq!(
            activitypub_collection_count(&serde_json::json!({
                "first": "https://remote.example/followers?page=1",
                "orderedItems": ["a"]
            })),
            None
        );
        assert_eq!(
            activitypub_collection_count(&serde_json::json!({
                "first": {
                    "id": "https://remote.example/followers?page=1",
                    "orderedItems": ["a", "b"]
                }
            })),
            None
        );
    }

    #[test]
    fn activitypub_collection_count_falls_back_to_items_without_first() {
        assert_eq!(
            activitypub_collection_count(&serde_json::json!({
                "orderedItems": ["a", "b", "c"]
            })),
            Some(3)
        );
    }

    #[test]
    fn from_remote_actor_uses_persisted_social_counts() {
        let actor = RemoteActorRow {
            actor_uri: "https://remote.example/users/alice".to_owned(),
            username: "alice".to_owned(),
            domain: "remote.example".to_owned(),
            created_at: "2026-01-02 03:04:05".to_owned(),
            locked: false,
            bot: false,
            discoverable: true,
            indexable: true,
            display_name: "Alice".to_owned(),
            summary_html: String::new(),
            profile_url: None,
            avatar_url: None,
            header_url: None,
            followers_count: 11,
            following_count: 22,
            statuses_count: 33,
            social_counts_updated_at: Some("2026-07-19 00:00:00".to_owned()),
        };
        let response = MastodonAccountResponse::from_remote_actor(&actor);
        assert_eq!(response.followers_count, 11);
        assert_eq!(response.following_count, 22);
        assert_eq!(response.statuses_count, 33);
    }

    #[test]
    fn from_remote_actor_sanitizes_stored_summary_html() {
        let actor = RemoteActorRow {
            actor_uri: "https://remote.example/users/alice".to_owned(),
            username: "alice".to_owned(),
            domain: "remote.example".to_owned(),
            created_at: String::new(),
            locked: false,
            bot: false,
            discoverable: true,
            indexable: true,
            display_name: "Alice".to_owned(),
            summary_html: "<p onclick=\"alert(1)\">hi <script>alert(2)</script></p>".to_owned(),
            profile_url: None,
            avatar_url: None,
            header_url: None,
            followers_count: 0,
            following_count: 0,
            statuses_count: 0,
            social_counts_updated_at: None,
        };
        let note = MastodonAccountResponse::from_remote_actor(&actor)
            .note
            .to_ascii_lowercase();
        assert!(note.contains("hi"));
        assert!(!note.contains("<script"));
        assert!(!note.contains("onclick"));
    }

    #[test]
    fn signed_fetch_allows_unsigned_fallback_for_auth_failures() {
        assert!(signed_fetch_allows_unsigned_fallback(&Error::RustError(
            "failed to fetch signed remote document https://x: HTTP 401".to_owned()
        )));
        assert!(signed_fetch_allows_unsigned_fallback(&Error::RustError(
            "account private signing key is missing".to_owned()
        )));
        assert!(signed_fetch_allows_unsigned_fallback(&Error::RustError(
            "failed to fetch signed remote document https://x: HTTP 523".to_owned()
        )));
        assert!(signed_fetch_allows_unsigned_fallback(&Error::RustError(
            "error decoding response body: expected value at line 1 column 1".to_owned()
        )));
    }

    #[test]
    fn remote_fetch_auth_mode_distinguishes_public_and_authorized() {
        assert_ne!(RemoteFetchAuthMode::Public, RemoteFetchAuthMode::Authorized);
    }

    #[test]
    fn remote_actor_social_counts_are_fresh_rejects_missing_timestamp() {
        assert!(!remote_actor_social_counts_are_fresh(None));
        assert!(!remote_actor_social_counts_are_fresh(Some("")));
        assert!(!remote_actor_social_counts_are_fresh(Some("not-a-date")));
    }

    #[test]
    fn apply_remote_actor_social_counts_only_updates_resolved_fields() {
        let mut account = MastodonAccountResponse::from_remote_actor(&RemoteActorRow {
            actor_uri: "https://remote.example/users/alice".to_owned(),
            username: "alice".to_owned(),
            domain: "remote.example".to_owned(),
            created_at: String::new(),
            locked: false,
            bot: false,
            discoverable: true,
            indexable: true,
            display_name: String::new(),
            summary_html: String::new(),
            profile_url: None,
            avatar_url: None,
            header_url: None,
            followers_count: 5,
            following_count: 6,
            statuses_count: 7,
            social_counts_updated_at: None,
        });
        apply_remote_actor_social_counts(
            &mut account,
            RemoteActorSocialCounts {
                followers_count: Some(50),
                following_count: None,
                statuses_count: Some(70),
            },
        );
        assert_eq!(account.followers_count, 50);
        assert_eq!(account.following_count, 6);
        assert_eq!(account.statuses_count, 70);
    }
}
