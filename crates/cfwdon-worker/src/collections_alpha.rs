use crate::notifications::{MastodonNotificationResponse, NotificationEntry};
use crate::{
    AccountReference, LocalApiAuthentication, MastodonAccountResponse, RemoteActorProfile,
    RemoteActorRow, Request, Response, Result, RouteContext, actor_url,
    app_bearer_token_from_request, authenticate_local_api_request,
    enqueue_targeted_outbox_activity, fetch_remote_activitypub_document, find_account_by_id,
    find_follow_by_target, find_remote_actor_by_actor_uri, generate_entity_id, instance_base_url,
    is_blocking_actor, list_follower_delivery_targets, load_account_stats, load_config,
    load_notification_policy_row, local_username_from_actor_uri, muted_notifications_for_actor,
    notification_account_matches_filter, notification_type_allowed,
    oauth_access_token_has_any_scope, parse_optional_bool, queue_remote_actor_activity,
    remote_account_rest_id, resolve_account_reference, timestamp_to_mastodon_iso8601,
    timestamp_to_mastodon_iso8601_opt, upsert_remote_actor,
};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use worker::d1::D1Type;

const DEFAULT_COLLECTIONS_LIMIT: u32 = 40;
const MAX_COLLECTIONS_LIMIT: u32 = 80;
const MAX_COLLECTION_NAME_LEN: usize = 40;
const MAX_COLLECTION_DESCRIPTION_LEN: usize = 100;
const MAX_COLLECTION_ITEMS: usize = 25;
const MAX_REMOTE_APPROVAL_REVALIDATIONS: i32 = 10;
const SUPPORTED_COLLECTION_LANGUAGES: &[&str] = &[
    "aa", "ab", "ae", "af", "ak", "am", "an", "ar", "as", "ast", "av", "ay", "az", "ba", "be",
    "bg", "bh", "bi", "bm", "bn", "bo", "br", "bs", "ca", "ce", "ch", "chr", "ckb", "cnr", "co",
    "cr", "cs", "csb", "cu", "cv", "cy", "da", "de", "dv", "dz", "ee", "el", "en", "eo", "es",
    "et", "eu", "fa", "ff", "fi", "fj", "fo", "fr", "fy", "ga", "gd", "gl", "gsw", "gu", "gv",
    "ha", "he", "hi", "ho", "hr", "ht", "hu", "hy", "hz", "ia", "id", "ie", "ig", "ii", "ik", "io",
    "is", "it", "iu", "ja", "jbo", "jv", "ka", "kab", "kg", "ki", "kj", "kk", "kl", "km", "kn",
    "ko", "kr", "ks", "ku", "kv", "kw", "ky", "la", "lb", "ldn", "lfn", "lg", "li", "ln", "lo",
    "lt", "lu", "lv", "mg", "mh", "mi", "mk", "ml", "mn", "mn-Mong", "moh", "mr", "ms", "ms-Arab",
    "mt", "my", "na", "nb", "nd", "nds", "ne", "ng", "nl", "nn", "no", "nr", "nv", "ny", "oc",
    "oj", "om", "or", "os", "pa", "pdc", "pi", "pl", "ps", "pt", "qu", "rm", "rn", "ro", "ru",
    "rw", "sa", "sc", "sco", "sd", "se", "sg", "si", "sk", "sl", "sma", "smj", "sn", "so", "sq",
    "sr", "ss", "st", "su", "sv", "sw", "szl", "ta", "te", "tg", "th", "ti", "tk", "tl", "tn",
    "to", "tok", "tr", "ts", "tt", "tw", "ty", "ug", "uk", "ur", "uz", "vai", "ve", "vi", "vo",
    "wa", "wo", "xal", "xh", "yi", "yo", "za", "zba", "zgh", "zh", "zh-CN", "zh-HK", "zh-TW",
    "zh-YUE", "zu",
];

#[derive(Debug, Default, Deserialize)]
struct CollectionsQuery {
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
struct CollectionRequest {
    name: Option<String>,
    description: Option<String>,
    language: Option<String>,
    sensitive: Option<bool>,
    discoverable: Option<bool>,
    tag_name: Option<String>,
    account_id: Option<String>,
    account_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct CollectionRow {
    id: String,
    account_id: String,
    name: String,
    description: String,
    language: Option<String>,
    sensitive: i32,
    discoverable: i32,
    tag_name: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CollectionItemRow {
    id: String,
    target_account_ref: String,
    state: String,
    activity_uri: Option<String>,
    feature_authorization: Option<String>,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct RemoteCollectionRow {
    id: String,
    actor_uri: String,
    uri: String,
    name: String,
    description: String,
    language: Option<String>,
    sensitive: i32,
    discoverable: i32,
    tag_name: Option<String>,
    url: Option<String>,
    published_at: Option<String>,
    remote_updated_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct RemoteCollectionItemRow {
    id: String,
    uri: Option<String>,
    target_actor_uri: String,
    state: String,
    feature_authorization: Option<String>,
    approval_last_verified_at: Option<String>,
    published_at: Option<String>,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct RemoteCollectionItemRevalidationRow {
    collection_id: String,
    collection_uri: String,
    target_actor_uri: String,
    feature_authorization: String,
}

#[derive(Debug, PartialEq, Eq)]
struct RemoteCollectionDraft {
    id: String,
    actor_uri: String,
    uri: String,
    name: String,
    description: String,
    language: Option<String>,
    sensitive: bool,
    discoverable: bool,
    tag_name: Option<String>,
    url: Option<String>,
    published_at: Option<String>,
    remote_updated_at: Option<String>,
    includes_items: bool,
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: u64,
}

#[derive(Debug, Deserialize)]
struct CollectionNotificationRow {
    id: String,
    from_account_id: String,
    collection_id: String,
    collection_item_id: Option<String>,
    notification_type: String,
    filtered: i32,
    created_at: String,
}

#[derive(Clone)]
struct CollectionViewer {
    account: Option<cfwdon_domain::LocalAccount>,
}

enum InCollectionPageEntry {
    Local(CollectionRow),
    Remote(RemoteCollectionRow),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CollectionNotificationPolicyAction {
    Deliver,
    Filter,
    Drop,
}

fn invalid_access_token_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "The access token is invalid",
    }))?
    .with_status(401))
}

fn outside_authorized_scopes_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "This action is outside the authorized scopes",
    }))?
    .with_status(403))
}

fn action_not_allowed_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "This action is not allowed",
    }))?
    .with_status(403))
}

fn validation_error_code(description: &str) -> &'static str {
    match description {
        "can't be blank" => "ERR_BLANK",
        "is invalid" => "ERR_INVALID",
        value if value.starts_with("is too long") => "ERR_TOO_LONG",
        value if value.starts_with("are too many") => "ERR_TOO_MANY",
        _ => "ERR_INVALID",
    }
}

fn validation_failed_response(details: BTreeMap<&'static str, Vec<String>>) -> Result<Response> {
    let mut messages = Vec::new();
    let mut formatted_details = serde_json::Map::new();
    for (field, field_errors) in &details {
        let label = match *field {
            "name" => "Name",
            "description" => "Description",
            "language" => "Language",
            "account_ids" => "Accounts",
            _ => field,
        };
        let mut formatted_errors = Vec::new();
        for error in field_errors {
            messages.push(format!("{label} {error}"));
            formatted_errors.push(serde_json::json!({
                "error": validation_error_code(error),
                "description": error,
            }));
        }
        formatted_details.insert(
            field.to_string(),
            serde_json::Value::Array(formatted_errors),
        );
    }
    Ok(Response::from_json(&serde_json::json!({
        "error": {
            "error": format!("Validation failed: {}", messages.join(", ")),
            "details": formatted_details,
        },
    }))?
    .with_status(422))
}

async fn optional_collection_viewer(
    req: &Request,
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
) -> Result<std::result::Result<CollectionViewer, Response>> {
    if app_bearer_token_from_request(req)?.is_some() {
        match authenticate_local_api_request(req, db, config).await? {
            LocalApiAuthentication::OAuthToken(auth) => {
                if !oauth_access_token_has_any_scope(&auth.token, &["read:collections", "read"]) {
                    return Ok(Err(outside_authorized_scopes_response()?));
                }
                return Ok(Ok(CollectionViewer {
                    account: Some(auth.account),
                }));
            }
            LocalApiAuthentication::AppToken | LocalApiAuthentication::InvalidBearer => {
                return Ok(Err(invalid_access_token_response()?));
            }
            LocalApiAuthentication::Auth0(account) => {
                return Ok(Ok(CollectionViewer {
                    account: Some(account),
                }));
            }
            LocalApiAuthentication::None => return Ok(Err(invalid_access_token_response()?)),
        }
    }

    let account = match authenticate_local_api_request(req, db, config).await? {
        LocalApiAuthentication::Auth0(account) => Some(account),
        LocalApiAuthentication::None => None,
        LocalApiAuthentication::OAuthToken(auth) => Some(auth.account),
        LocalApiAuthentication::AppToken | LocalApiAuthentication::InvalidBearer => {
            return Ok(Err(invalid_access_token_response()?));
        }
    };
    Ok(Ok(CollectionViewer { account }))
}

async fn require_collection_reader(
    req: &Request,
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
) -> Result<std::result::Result<cfwdon_domain::LocalAccount, Response>> {
    match authenticate_local_api_request(req, db, config).await? {
        LocalApiAuthentication::Auth0(account) => Ok(Ok(account)),
        LocalApiAuthentication::OAuthToken(auth) => {
            if !oauth_access_token_has_any_scope(&auth.token, &["read:collections", "read"]) {
                return Ok(Err(outside_authorized_scopes_response()?));
            }
            Ok(Ok(auth.account))
        }
        LocalApiAuthentication::AppToken
        | LocalApiAuthentication::InvalidBearer
        | LocalApiAuthentication::None => Ok(Err(invalid_access_token_response()?)),
    }
}

async fn require_collection_writer(
    req: &Request,
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
) -> Result<std::result::Result<cfwdon_domain::LocalAccount, Response>> {
    match authenticate_local_api_request(req, db, config).await? {
        LocalApiAuthentication::Auth0(account) => Ok(Ok(account)),
        LocalApiAuthentication::OAuthToken(auth) => {
            if !oauth_access_token_has_any_scope(&auth.token, &["write:collections", "write"]) {
                return Ok(Err(outside_authorized_scopes_response()?));
            }
            Ok(Ok(auth.account))
        }
        LocalApiAuthentication::AppToken
        | LocalApiAuthentication::InvalidBearer
        | LocalApiAuthentication::None => Ok(Err(invalid_access_token_response()?)),
    }
}

fn is_owner(viewer: Option<&cfwdon_domain::LocalAccount>, account_id: &str) -> bool {
    viewer.is_some_and(|viewer| viewer.id() == account_id)
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalize_optional_language(value: Option<String>) -> Option<String> {
    normalize_optional_text(value).map(|value| canonical_collection_language(&value))
}

fn normalize_optional_tag(value: Option<String>) -> Option<String> {
    normalize_optional_text(value)
        .map(|value| value.trim_start_matches('#').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn canonical_collection_language(value: &str) -> String {
    match value.trim().replace('_', "-").to_ascii_lowercase().as_str() {
        "mn-mong" => "mn-Mong".to_owned(),
        "ms-arab" => "ms-Arab".to_owned(),
        "zh-cn" => "zh-CN".to_owned(),
        "zh-hk" => "zh-HK".to_owned(),
        "zh-tw" => "zh-TW".to_owned(),
        "zh-yue" => "zh-YUE".to_owned(),
        value => value.to_owned(),
    }
}

fn collection_language_is_supported(value: &str) -> bool {
    SUPPORTED_COLLECTION_LANGUAGES.contains(&value)
}

async fn parse_collection_request(
    req: &mut Request,
) -> std::result::Result<CollectionRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut request = if content_type.contains("application/json") {
        req.json::<CollectionRequest>()
            .await
            .map_err(|error| format!("invalid collection JSON payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid collection form payload: {error}"))?;
        let account_ids = form.get_all("account_ids[]").map(|entries| {
            entries
                .into_iter()
                .filter_map(|entry| match entry {
                    worker::FormEntry::Field(value) => normalize_optional_text(Some(value)),
                    worker::FormEntry::File(_) => None,
                })
                .collect::<Vec<_>>()
        });
        CollectionRequest {
            name: form.get_field("name"),
            description: form.get_field("description"),
            language: form.get_field("language"),
            sensitive: parse_optional_bool(form.get_field("sensitive").as_deref())?,
            discoverable: parse_optional_bool(form.get_field("discoverable").as_deref())?,
            tag_name: form.get_field("tag_name"),
            account_id: form.get_field("account_id"),
            account_ids,
        }
    };

    request.name = normalize_optional_text(request.name);
    request.description = normalize_optional_text(request.description);
    request.language = normalize_optional_language(request.language);
    request.tag_name = normalize_optional_tag(request.tag_name);
    request.account_id = normalize_optional_text(request.account_id);
    if let Some(account_ids) = request.account_ids.as_mut() {
        account_ids.retain(|value| !value.trim().is_empty());
        account_ids.sort();
        account_ids.dedup();
    }
    Ok(request)
}

fn validate_collection_request(
    request: &CollectionRequest,
    require_name: bool,
) -> BTreeMap<&'static str, Vec<String>> {
    let mut details = BTreeMap::new();
    match request.name.as_deref() {
        None if require_name => {
            details.insert("name", vec!["can't be blank".to_owned()]);
        }
        Some(name) if name.chars().count() > MAX_COLLECTION_NAME_LEN => {
            details.insert(
                "name",
                vec![format!(
                    "is too long (maximum is {MAX_COLLECTION_NAME_LEN} characters)"
                )],
            );
        }
        _ => {}
    }
    if let Some(description) = request.description.as_deref()
        && description.chars().count() > MAX_COLLECTION_DESCRIPTION_LEN
    {
        details.insert(
            "description",
            vec![format!(
                "is too long (maximum is {MAX_COLLECTION_DESCRIPTION_LEN} characters)"
            )],
        );
    }
    if let Some(language) = request.language.as_deref()
        && !collection_language_is_supported(language)
    {
        details.insert("language", vec!["is invalid".to_owned()]);
    }
    if request
        .account_ids
        .as_ref()
        .is_some_and(|ids| ids.len() > MAX_COLLECTION_ITEMS)
    {
        details.insert(
            "account_ids",
            vec![format!("are too many (maximum is {MAX_COLLECTION_ITEMS})")],
        );
    }
    details
}

fn collection_update_is_significant(existing: &CollectionRow, request: &CollectionRequest) -> bool {
    request
        .name
        .as_ref()
        .is_some_and(|value| value != &existing.name)
        || request
            .description
            .as_ref()
            .is_some_and(|value| value != &existing.description)
        || request
            .sensitive
            .is_some_and(|value| value != (existing.sensitive != 0))
        || request
            .tag_name
            .as_ref()
            .is_some_and(|value| Some(value) != existing.tag_name.as_ref())
}

fn collection_update_requires_activity(
    existing: &CollectionRow,
    request: &CollectionRequest,
) -> bool {
    collection_update_is_significant(existing, request)
        || request
            .language
            .as_ref()
            .is_some_and(|value| Some(value) != existing.language.as_ref())
        || request
            .discoverable
            .is_some_and(|value| value != (existing.discoverable != 0))
}

async fn collection_row_by_id(
    db: &crate::D1Database,
    collection_id: &str,
) -> Result<Option<CollectionRow>> {
    let collection_id = D1Type::Text(collection_id);
    db.prepare(
        "SELECT c.id,
                c.account_id,
                c.name,
                c.description,
                c.language,
                c.sensitive,
                c.discoverable,
                c.tag_name,
                c.created_at,
                c.updated_at
         FROM account_collections c
         WHERE c.id = ?1
         LIMIT 1",
    )
    .bind_refs(&collection_id)?
    .first::<CollectionRow>(None)
    .await
}

async fn list_collection_rows_for_account(
    db: &crate::D1Database,
    account_id: &str,
    include_private: bool,
    offset: u32,
    limit: u32,
) -> Result<Vec<CollectionRow>> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Integer(if include_private { 1 } else { 0 }),
        D1Type::Integer(i32::try_from(limit).unwrap_or(i32::MAX)),
        D1Type::Integer(i32::try_from(offset).unwrap_or(i32::MAX)),
    ];
    let result = db
        .prepare(
            "SELECT c.id,
                    c.account_id,
                    c.name,
                    c.description,
                    c.language,
                    c.sensitive,
                    c.discoverable,
                    c.tag_name,
                    c.created_at,
                    c.updated_at
             FROM account_collections c
             WHERE c.account_id = ?1
               AND (?2 = 1 OR c.discoverable = 1)
             ORDER BY c.created_at DESC, c.id DESC
             LIMIT ?3 OFFSET ?4",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    result.results::<CollectionRow>()
}

async fn count_collection_rows_for_account(
    db: &crate::D1Database,
    account_id: &str,
    include_private: bool,
) -> Result<u64> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Integer(if include_private { 1 } else { 0 }),
    ];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM account_collections c
             WHERE c.account_id = ?1
               AND (?2 = 1 OR c.discoverable = 1)",
        )
        .bind_refs(bindings.iter())?
        .first::<CountRow>(None)
        .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

async fn count_in_collection_rows(db: &crate::D1Database, account_id: &str) -> Result<u64> {
    let bindings = [D1Type::Text(account_id)];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM account_collections c
             JOIN account_collection_items target_item
               ON target_item.collection_id = c.id
              AND target_item.target_account_ref = ?1
              AND target_item.state IN ('accepted', 'pending')",
        )
        .bind_refs(bindings.iter())?
        .first::<CountRow>(None)
        .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

async fn remote_collection_row_by_id(
    db: &crate::D1Database,
    collection_id: &str,
) -> Result<Option<RemoteCollectionRow>> {
    let collection_id = D1Type::Text(collection_id);
    db.prepare(
        "SELECT id,
                actor_uri,
                uri,
                name,
                description,
                language,
                sensitive,
                discoverable,
                tag_name,
                url,
                published_at,
                remote_updated_at,
                created_at,
                updated_at
         FROM remote_account_collections
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&collection_id)?
    .first::<RemoteCollectionRow>(None)
    .await
}

async fn remote_collection_row_by_uri(
    db: &crate::D1Database,
    collection_uri: &str,
) -> Result<Option<RemoteCollectionRow>> {
    let collection_uri = D1Type::Text(collection_uri);
    db.prepare(
        "SELECT id,
                actor_uri,
                uri,
                name,
                description,
                language,
                sensitive,
                discoverable,
                tag_name,
                url,
                published_at,
                remote_updated_at,
                created_at,
                updated_at
         FROM remote_account_collections
         WHERE uri = ?1
         LIMIT 1",
    )
    .bind_refs(&collection_uri)?
    .first::<RemoteCollectionRow>(None)
    .await
}

async fn list_remote_collection_rows_for_actor(
    db: &crate::D1Database,
    actor_uri: &str,
    offset: u32,
    limit: u32,
) -> Result<Vec<RemoteCollectionRow>> {
    let bindings = [
        D1Type::Text(actor_uri),
        D1Type::Integer(i32::try_from(limit).unwrap_or(i32::MAX)),
        D1Type::Integer(i32::try_from(offset).unwrap_or(i32::MAX)),
    ];
    let result = db
        .prepare(
            "SELECT id,
                    actor_uri,
                    uri,
                    name,
                    description,
                    language,
                    sensitive,
                    discoverable,
                    tag_name,
                    url,
                    published_at,
                    remote_updated_at,
                    created_at,
                    updated_at
             FROM remote_account_collections
             WHERE actor_uri = ?1
               AND discoverable = 1
             ORDER BY created_at DESC, id DESC
             LIMIT ?2 OFFSET ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    result.results::<RemoteCollectionRow>()
}

async fn count_remote_collection_rows_for_actor(
    db: &crate::D1Database,
    actor_uri: &str,
) -> Result<u64> {
    let actor_uri = D1Type::Text(actor_uri);
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM remote_account_collections
             WHERE actor_uri = ?1
               AND discoverable = 1",
        )
        .bind_refs(&actor_uri)?
        .first::<CountRow>(None)
        .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

async fn count_remote_in_collection_rows(
    db: &crate::D1Database,
    target_actor_uri: &str,
) -> Result<u64> {
    let target_actor_uri = D1Type::Text(target_actor_uri);
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM remote_account_collections c
             JOIN remote_account_collection_items target_item
               ON target_item.collection_id = c.id
              AND target_item.target_actor_uri = ?1
              AND target_item.state IN ('accepted', 'pending')",
        )
        .bind_refs(&target_actor_uri)?
        .first::<CountRow>(None)
        .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

async fn list_collection_items(
    db: &crate::D1Database,
    collection_id: &str,
    include_pending: bool,
) -> Result<Vec<CollectionItemRow>> {
    let bindings = [
        D1Type::Text(collection_id),
        D1Type::Integer(if include_pending { 1 } else { 0 }),
    ];
    let result = db
        .prepare(
            "SELECT id,
                    target_account_ref,
                    state,
                    activity_uri,
                    feature_authorization,
                    created_at
             FROM account_collection_items
             WHERE collection_id = ?1
               AND state IN ('accepted', CASE WHEN ?2 = 1 THEN 'pending' ELSE 'accepted' END)
             ORDER BY created_at ASC, id ASC",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    result.results::<CollectionItemRow>()
}

async fn list_remote_collection_items(
    db: &crate::D1Database,
    collection_id: &str,
    include_pending: bool,
) -> Result<Vec<RemoteCollectionItemRow>> {
    let bindings = [
        D1Type::Text(collection_id),
        D1Type::Integer(if include_pending { 1 } else { 0 }),
    ];
    let result = db
        .prepare(
            "SELECT id,
                    uri,
                    target_actor_uri,
                    state,
                    feature_authorization,
                    approval_last_verified_at,
                    published_at,
                    created_at
             FROM remote_account_collection_items
             WHERE collection_id = ?1
               AND state IN ('accepted', CASE WHEN ?2 = 1 THEN 'pending' ELSE 'accepted' END)
             ORDER BY created_at ASC, id ASC",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    result.results::<RemoteCollectionItemRow>()
}

async fn remote_collection_item_by_id(
    db: &crate::D1Database,
    collection_id: &str,
    item_id: &str,
) -> Result<Option<RemoteCollectionItemRow>> {
    db.prepare(
        "SELECT id,
                uri,
                target_actor_uri,
                state,
                feature_authorization,
                approval_last_verified_at,
                published_at,
                created_at
         FROM remote_account_collection_items
         WHERE collection_id = ?1
           AND id = ?2
         LIMIT 1",
    )
    .bind_refs(&[D1Type::Text(collection_id), D1Type::Text(item_id)])?
    .first::<RemoteCollectionItemRow>(None)
    .await
}

async fn list_remote_collection_items_due_for_approval_revalidation(
    db: &crate::D1Database,
    collection_id: &str,
) -> Result<Vec<RemoteCollectionItemRow>> {
    let bindings = [
        D1Type::Text(collection_id),
        D1Type::Integer(MAX_REMOTE_APPROVAL_REVALIDATIONS),
    ];
    let result = db
        .prepare(
            "SELECT id,
                    uri,
                    target_actor_uri,
                    state,
                    feature_authorization,
                    approval_last_verified_at,
                    published_at,
                    created_at
             FROM remote_account_collection_items
             WHERE collection_id = ?1
               AND state = 'accepted'
               AND feature_authorization IS NOT NULL
               AND (
                    approval_last_verified_at IS NULL
                    OR approval_last_verified_at <= datetime('now', '-1 day')
               )
             ORDER BY COALESCE(approval_last_verified_at, created_at) ASC, id ASC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    result.results::<RemoteCollectionItemRow>()
}

async fn list_stale_remote_collection_items_for_approval_revalidation(
    db: &crate::D1Database,
    limit: i32,
) -> Result<Vec<RemoteCollectionItemRevalidationRow>> {
    let result = db
        .prepare(
            "SELECT item.collection_id AS collection_id,
                    collection.uri AS collection_uri,
                    item.target_actor_uri AS target_actor_uri,
                    item.feature_authorization AS feature_authorization
             FROM remote_account_collection_items item
             JOIN remote_account_collections collection
               ON collection.id = item.collection_id
             WHERE item.state = 'accepted'
               AND item.feature_authorization IS NOT NULL
               AND (
                    item.approval_last_verified_at IS NULL
                    OR item.approval_last_verified_at <= datetime('now', '-1 day')
               )
             ORDER BY COALESCE(item.approval_last_verified_at, item.created_at) ASC, item.id ASC
             LIMIT ?1",
        )
        .bind_refs(&[D1Type::Integer(limit)])?
        .all()
        .await?;
    result.results::<RemoteCollectionItemRevalidationRow>()
}

async fn list_remote_in_collection_rows(
    db: &crate::D1Database,
    target_actor_uri: &str,
    limit: u32,
) -> Result<Vec<RemoteCollectionRow>> {
    let bindings = [
        D1Type::Text(target_actor_uri),
        D1Type::Integer(i32::try_from(limit).unwrap_or(i32::MAX)),
    ];
    let result = db
        .prepare(
            "SELECT c.id,
                    c.actor_uri,
                    c.uri,
                    c.name,
                    c.description,
                    c.language,
                    c.sensitive,
                    c.discoverable,
                    c.tag_name,
                    c.url,
                    c.published_at,
                    c.remote_updated_at,
                    c.created_at,
                    c.updated_at
             FROM remote_account_collections c
             JOIN remote_account_collection_items target_item
              ON target_item.collection_id = c.id
             AND target_item.target_actor_uri = ?1
             AND target_item.state IN ('accepted', 'pending')
             ORDER BY c.created_at DESC, c.id DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    result.results::<RemoteCollectionRow>()
}

async fn list_local_in_collection_rows(
    db: &crate::D1Database,
    target_account_id: &str,
    limit: u32,
) -> Result<Vec<CollectionRow>> {
    let bindings = [
        D1Type::Text(target_account_id),
        D1Type::Integer(i32::try_from(limit).unwrap_or(i32::MAX)),
    ];
    let result = db
        .prepare(
            "SELECT c.id,
                    c.account_id,
                    c.name,
                    c.description,
                    c.language,
                    c.sensitive,
                    c.discoverable,
                    c.tag_name,
                    c.created_at,
                    c.updated_at
             FROM account_collections c
             JOIN account_collection_items target_item
               ON target_item.collection_id = c.id
              AND target_item.target_account_ref = ?1
              AND target_item.state IN ('accepted', 'pending')
             ORDER BY c.created_at DESC, c.id DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    result.results::<CollectionRow>()
}

fn in_collection_entry_sort_key(entry: &InCollectionPageEntry) -> (&str, &str) {
    match entry {
        InCollectionPageEntry::Local(row) => (&row.created_at, &row.id),
        InCollectionPageEntry::Remote(row) => (&row.created_at, &row.id),
    }
}

fn sort_in_collection_page_entries(entries: &mut [InCollectionPageEntry]) {
    entries.sort_by(|left, right| {
        let (left_created_at, left_id) = in_collection_entry_sort_key(left);
        let (right_created_at, right_id) = in_collection_entry_sort_key(right);
        right_created_at
            .cmp(left_created_at)
            .then_with(|| right_id.cmp(left_id))
    });
}

fn tag_document(config: &cfwdon_core::AppConfig, tag_name: Option<&str>) -> serde_json::Value {
    let Some(tag_name) = tag_name else {
        return serde_json::Value::Null;
    };
    serde_json::json!({
        "name": tag_name,
        "url": format!("{}/tags/{}", instance_base_url(config), tag_name),
    })
}

fn collection_uri(
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
) -> String {
    format!(
        "{}/collections/{collection_id}",
        actor_url(config, owner.username())
    )
}

fn collection_item_uri(
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item_id: &str,
) -> String {
    format!(
        "{}/items/{item_id}",
        collection_uri(config, owner, collection_id)
    )
}

pub(crate) fn local_collection_id_from_uri(
    config: &cfwdon_core::AppConfig,
    uri: &str,
) -> Option<String> {
    let base = format!("{}/users/", instance_base_url(config));
    let rest = uri.strip_prefix(&base)?;
    let (_, collection_id) = rest.split_once("/collections/")?;
    collection_id
        .split('/')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn collection_document(
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    row: &CollectionRow,
    items: Vec<serde_json::Value>,
) -> serde_json::Value {
    let uri = collection_uri(config, owner, &row.id);
    serde_json::json!({
        "id": row.id,
        "uri": uri,
        "name": row.name,
        "description": row.description,
        "language": row.language,
        "account_id": row.account_id,
        "local": true,
        "sensitive": row.sensitive,
        "discoverable": row.discoverable != 0,
        "url": uri,
        "item_count": items.len(),
        "created_at": timestamp_to_mastodon_iso8601(&row.created_at),
        "updated_at": timestamp_to_mastodon_iso8601(&row.updated_at),
        "tag": tag_document(config, row.tag_name.as_deref()),
        "items": items,
    })
}

fn remote_collection_document(
    config: &cfwdon_core::AppConfig,
    owner: &RemoteActorRow,
    row: &RemoteCollectionRow,
    items: Vec<serde_json::Value>,
) -> serde_json::Value {
    let uri = row.url.as_deref().unwrap_or(&row.uri);
    let created_at = row.published_at.as_deref().unwrap_or(&row.created_at);
    let updated_at = row.remote_updated_at.as_deref().unwrap_or(&row.updated_at);
    serde_json::json!({
        "id": row.id,
        "uri": row.uri,
        "name": row.name,
        "description": row.description,
        "language": row.language,
        "account_id": remote_account_rest_id(&owner.actor_uri),
        "local": false,
        "sensitive": row.sensitive,
        "discoverable": row.discoverable != 0,
        "url": uri,
        "item_count": items.len(),
        "created_at": timestamp_to_mastodon_iso8601(created_at),
        "updated_at": timestamp_to_mastodon_iso8601(updated_at),
        "tag": tag_document(config, row.tag_name.as_deref()),
        "items": items,
    })
}

fn collection_list_document(collections: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({ "collections": collections })
}

fn collection_response_document(collection: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "collection": collection })
}

async fn account_actor_uri_for_reference(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    account_ref: &str,
) -> Result<Option<String>> {
    match resolve_account_reference(db, account_ref).await? {
        Some(AccountReference::Local(account)) => Ok(Some(actor_url(config, account.username()))),
        Some(AccountReference::Remote(actor)) => Ok(Some(actor.actor_uri)),
        None => Ok(None),
    }
}

async fn collection_item_activitypub_object(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item: &CollectionItemRow,
) -> Result<Option<serde_json::Value>> {
    let Some(featured_object) =
        account_actor_uri_for_reference(db, config, &item.target_account_ref).await?
    else {
        return Ok(None);
    };
    let item_uri = collection_item_uri(config, owner, collection_id, &item.id);
    let feature_authorization = item
        .feature_authorization
        .clone()
        .unwrap_or_else(|| format!("{item_uri}/feature_authorization"));
    Ok(Some(serde_json::json!({
        "id": item_uri,
        "type": "FeaturedItem",
        "featuredObject": featured_object,
        "featuredObjectType": "Person",
        "featureAuthorization": feature_authorization,
        "published": timestamp_to_mastodon_iso8601(&item.created_at),
    })))
}

async fn collection_activitypub_object(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    row: &CollectionRow,
) -> Result<serde_json::Value> {
    let item_rows = list_collection_items(db, &row.id, false).await?;
    let mut ordered_items = Vec::new();
    for item in &item_rows {
        if let Some(object) =
            collection_item_activitypub_object(db, config, owner, &row.id, item).await?
        {
            ordered_items.push(object);
        }
    }

    let uri = collection_uri(config, owner, &row.id);
    let mut object = serde_json::json!({
        "id": uri,
        "type": "FeaturedCollection",
        "totalItems": ordered_items.len(),
        "name": row.name,
        "attributedTo": actor_url(config, owner.username()),
        "url": uri,
        "sensitive": row.sensitive,
        "discoverable": row.discoverable != 0,
        "published": timestamp_to_mastodon_iso8601(&row.created_at),
        "updated": timestamp_to_mastodon_iso8601(&row.updated_at),
        "orderedItems": ordered_items,
    });
    if let Some(language) = row.language.as_deref().filter(|value| !value.is_empty()) {
        object["summaryMap"] = serde_json::json!({ language: row.description });
    } else {
        object["summary"] = serde_json::json!(row.description);
    }
    if let Some(tag_name) = row.tag_name.as_deref().filter(|value| !value.is_empty()) {
        object["topic"] = serde_json::json!({
            "type": "Hashtag",
            "name": format!("#{tag_name}"),
            "href": format!("{}/tags/{tag_name}", instance_base_url(config)),
        });
    }
    Ok(object)
}

async fn enqueue_collection_followers_activity(
    db: &crate::D1Database,
    _config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    _collection_id: &str,
    payload: serde_json::Value,
) -> Result<()> {
    let follower_inboxes = list_follower_delivery_targets(db, owner.id()).await?;
    if follower_inboxes.is_empty() {
        return Ok(());
    }
    let payload_json = serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize collection activity: {error}"))
    })?;
    enqueue_targeted_outbox_activity(db, owner.id(), None, &payload_json, &follower_inboxes).await
}

async fn enqueue_collection_add_activity(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    row: &CollectionRow,
) -> Result<()> {
    let actor = actor_url(config, owner.username());
    let collection_uri = collection_uri(config, owner, &row.id);
    let payload = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{collection_uri}#add"),
        "type": "Add",
        "actor": actor,
        "object": collection_activitypub_object(db, config, owner, row).await?,
        "target": format!("{actor}/collections/featured"),
        "to": [format!("{actor}/followers")],
    });
    enqueue_collection_followers_activity(db, config, owner, &row.id, payload).await
}

async fn enqueue_collection_update_activity(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    row: &CollectionRow,
) -> Result<()> {
    let collection_uri = collection_uri(config, owner, &row.id);
    let payload = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{collection_uri}#updates/{}", row.updated_at),
        "type": "Update",
        "actor": actor_url(config, owner.username()),
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "object": collection_activitypub_object(db, config, owner, row).await?,
    });
    enqueue_collection_followers_activity(db, config, owner, &row.id, payload).await
}

async fn enqueue_collection_remove_activity(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    row: &CollectionRow,
) -> Result<()> {
    let actor = actor_url(config, owner.username());
    let collection_uri = collection_uri(config, owner, &row.id);
    let payload = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{collection_uri}#remove"),
        "type": "Remove",
        "actor": actor,
        "object": collection_uri,
        "target": format!("{actor}/collections/featured"),
        "to": [format!("{actor}/followers")],
    });
    enqueue_collection_followers_activity(db, config, owner, &row.id, payload).await
}

async fn enqueue_collection_item_add_activity(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item: &CollectionItemRow,
) -> Result<()> {
    let Some(object) =
        collection_item_activitypub_object(db, config, owner, collection_id, item).await?
    else {
        return Ok(());
    };
    let item_uri = collection_item_uri(config, owner, collection_id, &item.id);
    let payload = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{item_uri}#add"),
        "type": "Add",
        "actor": actor_url(config, owner.username()),
        "object": object,
        "target": collection_uri(config, owner, collection_id),
    });
    enqueue_collection_followers_activity(db, config, owner, collection_id, payload).await
}

fn build_collection_feature_request_activity(
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item: &CollectionItemRow,
    remote_actor_uri: &str,
) -> serde_json::Value {
    serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": collection_feature_request_uri(config, owner, collection_id, &item.id),
        "type": "FeatureRequest",
        "object": remote_actor_uri,
        "instrument": collection_uri(config, owner, collection_id),
    })
}

fn collection_feature_request_uri(
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item_id: &str,
) -> String {
    format!(
        "{}#feature_request",
        collection_item_uri(config, owner, collection_id, item_id)
    )
}

async fn enqueue_collection_feature_request_activity(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item: &CollectionItemRow,
    remote_actor_uri: &str,
) -> Result<()> {
    let activity_uri = collection_feature_request_uri(config, owner, collection_id, &item.id);
    update_collection_item_feature_request_uri(db, collection_id, &item.id, &activity_uri).await?;
    let payload = build_collection_feature_request_activity(
        config,
        owner,
        collection_id,
        item,
        remote_actor_uri,
    );
    let payload_json = serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize collection feature request: {error}"
        ))
    })?;
    let _ = queue_remote_actor_activity(db, owner.id(), remote_actor_uri, &payload_json).await?;
    Ok(())
}

async fn enqueue_collection_item_remove_activity(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item: &CollectionItemRow,
) -> Result<()> {
    let item_uri = collection_item_uri(config, owner, collection_id, &item.id);
    let payload = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{item_uri}#remove"),
        "type": "Remove",
        "actor": actor_url(config, owner.username()),
        "object": item_uri,
        "target": collection_uri(config, owner, collection_id),
    });
    enqueue_collection_followers_activity(db, config, owner, collection_id, payload).await
}

async fn account_blocks_viewer(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    account: &cfwdon_domain::LocalAccount,
    viewer: Option<&cfwdon_domain::LocalAccount>,
) -> Result<bool> {
    let Some(viewer) = viewer else {
        return Ok(false);
    };
    if viewer.id() == account.id() {
        return Ok(false);
    }
    is_blocking_actor(db, account.id(), &actor_url(config, viewer.username())).await
}

async fn owner_follows_actor(
    db: &crate::D1Database,
    owner: &cfwdon_domain::LocalAccount,
    target_actor_uri: &str,
) -> Result<bool> {
    Ok(find_follow_by_target(db, owner.id(), target_actor_uri)
        .await?
        .map(|follow| follow.state == "accepted")
        .unwrap_or(false))
}

async fn account_reference_featureable_by_owner(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    target: &AccountReference,
) -> Result<bool> {
    match target {
        AccountReference::Local(target) => {
            if !target.is_discoverable() {
                return Ok(false);
            }
            let target_actor_uri = actor_url(config, target.username());
            if is_blocking_actor(db, owner.id(), &target_actor_uri).await?
                || is_blocking_actor(db, target.id(), &actor_url(config, owner.username())).await?
            {
                return Ok(false);
            }
            if target.is_locked() && target.id() != owner.id() {
                return owner_follows_actor(db, owner, &target_actor_uri).await;
            }
            Ok(true)
        }
        AccountReference::Remote(target) => {
            if !target.discoverable || is_blocking_actor(db, owner.id(), &target.actor_uri).await? {
                return Ok(false);
            }
            if target.locked {
                return owner_follows_actor(db, owner, &target.actor_uri).await;
            }
            Ok(true)
        }
    }
}

fn collection_item_document(row: &CollectionItemRow) -> serde_json::Value {
    let mut value = serde_json::json!({
        "id": row.id,
        "state": row.state,
        "created_at": timestamp_to_mastodon_iso8601(&row.created_at),
    });
    if row.state == "accepted" || row.state == "pending" {
        value["account_id"] = serde_json::json!(row.target_account_ref);
    }
    if let Some(activity_uri) = row.activity_uri.as_deref() {
        value["activity_uri"] = serde_json::json!(activity_uri);
    }
    if let Some(feature_authorization) = row.feature_authorization.as_deref() {
        value["feature_authorization"] = serde_json::json!(feature_authorization);
    }
    value
}

async fn account_id_for_actor_uri(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    actor_uri: &str,
) -> Result<String> {
    if let Some(username) = local_username_from_actor_uri(config, actor_uri)
        && let Some(account) = crate::find_account_by_username(db, &username).await?
    {
        return Ok(account.id().to_owned());
    }
    Ok(remote_account_rest_id(actor_uri))
}

async fn remote_collection_item_document(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    row: &RemoteCollectionItemRow,
) -> Result<serde_json::Value> {
    let mut value = serde_json::json!({
        "id": row.id,
        "uri": row.uri,
        "state": row.state,
        "created_at": timestamp_to_mastodon_iso8601(
            row.published_at.as_deref().unwrap_or(&row.created_at),
        ),
        "feature_authorization": row.feature_authorization,
        "approval_last_verified_at": timestamp_to_mastodon_iso8601_opt(
            row.approval_last_verified_at.as_deref(),
        ),
    });
    if row.state == "accepted" || row.state == "pending" {
        value["account_id"] =
            serde_json::json!(account_id_for_actor_uri(db, config, &row.target_actor_uri).await?);
    }
    Ok(value)
}

fn collection_item_response_document(collection_item: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "collection_item": collection_item })
}

async fn account_response_for_reference(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    account_ref: &str,
) -> Result<Option<MastodonAccountResponse>> {
    match resolve_account_reference(db, account_ref).await? {
        Some(AccountReference::Local(account)) => {
            let stats = load_account_stats(db, account.id()).await?;
            Ok(Some(MastodonAccountResponse::from_account_with_stats(
                &account, config, &stats,
            )))
        }
        Some(AccountReference::Remote(actor)) => {
            Ok(Some(MastodonAccountResponse::from_remote_actor(&actor)))
        }
        None => Ok(None),
    }
}

async fn remote_account_response_for_actor_uri(
    db: &crate::D1Database,
    actor_uri: &str,
) -> Result<Option<MastodonAccountResponse>> {
    Ok(find_remote_actor_by_actor_uri(db, actor_uri)
        .await?
        .map(|actor| MastodonAccountResponse::from_remote_actor(&actor)))
}

async fn collection_item_visible_to_viewer(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    item: &CollectionItemRow,
    viewer: Option<&cfwdon_domain::LocalAccount>,
) -> Result<bool> {
    let Some(viewer) = viewer else {
        return Ok(true);
    };
    let target_actor_uri = match resolve_account_reference(db, &item.target_account_ref).await? {
        Some(AccountReference::Local(account)) => actor_url(config, account.username()),
        Some(AccountReference::Remote(actor)) => actor.actor_uri,
        None => return Ok(true),
    };
    Ok(!is_blocking_actor(db, viewer.id(), &target_actor_uri).await?)
}

async fn collection_with_accounts_document(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    row: &CollectionRow,
    include_pending: bool,
    viewer: Option<&cfwdon_domain::LocalAccount>,
) -> Result<serde_json::Value> {
    let item_rows = list_collection_items(db, &row.id, include_pending).await?;
    let mut visible_item_rows = Vec::new();
    for item in item_rows {
        if collection_item_visible_to_viewer(db, config, &item, viewer).await? {
            visible_item_rows.push(item);
        }
    }

    let items = visible_item_rows
        .iter()
        .map(collection_item_document)
        .collect::<Vec<_>>();
    let collection = collection_document(config, owner, row, items);

    let mut accounts = Vec::new();
    let stats = load_account_stats(db, owner.id()).await?;
    accounts.push(MastodonAccountResponse::from_account_with_stats(
        owner, config, &stats,
    ));

    let mut seen = HashSet::from([owner.id().to_owned()]);
    for item in visible_item_rows {
        if !seen.insert(item.target_account_ref.clone()) {
            continue;
        }
        if let Some(account) =
            account_response_for_reference(db, config, &item.target_account_ref).await?
        {
            accounts.push(account);
        }
    }

    Ok(serde_json::json!({
        "collection": collection,
        "accounts": accounts,
    }))
}

async fn remote_collection_item_visible_to_viewer(
    db: &crate::D1Database,
    item: &RemoteCollectionItemRow,
    viewer: Option<&cfwdon_domain::LocalAccount>,
) -> Result<bool> {
    let Some(viewer) = viewer else {
        return Ok(true);
    };
    Ok(!is_blocking_actor(db, viewer.id(), &item.target_actor_uri).await?)
}

async fn remote_collection_with_accounts_document(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &RemoteActorRow,
    row: &RemoteCollectionRow,
    include_pending: bool,
    viewer: Option<&cfwdon_domain::LocalAccount>,
) -> Result<serde_json::Value> {
    revalidate_remote_collection_item_approvals(db, config, row).await?;
    let item_rows = list_remote_collection_items(db, &row.id, include_pending).await?;
    let mut visible_item_rows = Vec::new();
    for item in item_rows {
        if remote_collection_item_visible_to_viewer(db, &item, viewer).await? {
            visible_item_rows.push(item);
        }
    }

    let mut items = Vec::new();
    for item in &visible_item_rows {
        items.push(remote_collection_item_document(db, config, item).await?);
    }
    let collection = remote_collection_document(config, owner, row, items);

    let mut accounts = Vec::new();
    accounts.push(MastodonAccountResponse::from_remote_actor(owner));

    let mut seen = HashSet::from([owner.actor_uri.clone()]);
    for item in visible_item_rows {
        if !seen.insert(item.target_actor_uri.clone()) {
            continue;
        }
        if let Some(username) = local_username_from_actor_uri(config, &item.target_actor_uri)
            && let Some(account) = crate::find_account_by_username(db, &username).await?
        {
            let stats = load_account_stats(db, account.id()).await?;
            accounts.push(MastodonAccountResponse::from_account_with_stats(
                &account, config, &stats,
            ));
        } else if let Some(account) =
            remote_account_response_for_actor_uri(db, &item.target_actor_uri).await?
        {
            accounts.push(account);
        }
    }

    Ok(serde_json::json!({
        "collection": collection,
        "accounts": accounts,
    }))
}

fn activitypub_value_id(value: Option<&serde_json::Value>) -> Option<&str> {
    match value {
        Some(serde_json::Value::String(value)) => Some(value.as_str()),
        Some(serde_json::Value::Object(map)) => map.get("id").and_then(serde_json::Value::as_str),
        _ => None,
    }
}

fn activitypub_collection_uri(actor_uri: &str, path: &str) -> String {
    format!("{}/collections/{path}", actor_uri.trim_end_matches('/'))
}

fn is_remote_actor_collections_target(actor_uri: &str, target: &str) -> bool {
    target == activitypub_collection_uri(actor_uri, "featured")
        || target == format!("{}/collections", actor_uri.trim_end_matches('/'))
}

fn collection_object_attributed_to_actor(object: &serde_json::Value, actor_uri: &str) -> bool {
    match object.get("attributedTo") {
        Some(serde_json::Value::String(value)) => value == actor_uri,
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .any(|value| activitypub_value_id(Some(value)) == Some(actor_uri)),
        Some(value) => activitypub_value_id(Some(value)) == Some(actor_uri),
        None => false,
    }
}

fn featured_collection_description(object: &serde_json::Value) -> (String, Option<String>) {
    if let Some(summary) = object.get("summary").and_then(serde_json::Value::as_str) {
        return (summary.to_owned(), None);
    }
    let Some(summary_map) = object
        .get("summaryMap")
        .and_then(serde_json::Value::as_object)
    else {
        return (String::new(), None);
    };
    summary_map
        .iter()
        .find_map(|(language, value)| {
            value
                .as_str()
                .map(|description| (description.to_owned(), Some(language.to_owned())))
        })
        .unwrap_or_default()
}

fn featured_collection_tag_name(object: &serde_json::Value) -> Option<String> {
    object
        .get("topic")
        .and_then(|topic| {
            topic
                .get("name")
                .or_else(|| topic.get("href"))
                .and_then(serde_json::Value::as_str)
        })
        .map(|value| value.trim().trim_start_matches('#').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn featured_collection_items(object: &serde_json::Value) -> Vec<&serde_json::Value> {
    object
        .get("orderedItems")
        .or_else(|| object.get("items"))
        .and_then(serde_json::Value::as_array)
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

fn remote_collection_draft_from_object(
    remote_actor: &RemoteActorProfile,
    object: &serde_json::Value,
) -> Option<RemoteCollectionDraft> {
    if object.get("type").and_then(serde_json::Value::as_str) != Some("FeaturedCollection")
        || !collection_object_attributed_to_actor(object, &remote_actor.actor_uri)
    {
        return None;
    }
    let collection_uri = activitypub_value_id(Some(object))?;
    let name = object
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let (description, language) = featured_collection_description(object);

    Some(RemoteCollectionDraft {
        id: remote_account_rest_id(collection_uri),
        actor_uri: remote_actor.actor_uri.clone(),
        uri: collection_uri.to_owned(),
        name: name.to_owned(),
        description,
        language,
        sensitive: object
            .get("sensitive")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        discoverable: object
            .get("discoverable")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        tag_name: featured_collection_tag_name(object),
        url: activitypub_value_id(object.get("url")).map(ToOwned::to_owned),
        published_at: object
            .get("published")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        remote_updated_at: object
            .get("updated")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        includes_items: object
            .get("orderedItems")
            .or_else(|| object.get("items"))
            .is_some(),
    })
}

async fn upsert_remote_collection_draft(
    db: &crate::D1Database,
    draft: &RemoteCollectionDraft,
) -> Result<()> {
    let bindings = [
        D1Type::Text(draft.id.as_str()),
        D1Type::Text(draft.actor_uri.as_str()),
        D1Type::Text(draft.uri.as_str()),
        D1Type::Text(draft.name.as_str()),
        D1Type::Text(draft.description.as_str()),
        draft.language.as_deref().map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(i32::from(draft.sensitive)),
        D1Type::Integer(i32::from(draft.discoverable)),
        draft.tag_name.as_deref().map_or(D1Type::Null, D1Type::Text),
        draft.url.as_deref().map_or(D1Type::Null, D1Type::Text),
        draft
            .published_at
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        draft
            .remote_updated_at
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
    ];
    db.prepare(
        "INSERT INTO remote_account_collections (
            id,
            actor_uri,
            uri,
            name,
            description,
            language,
            sensitive,
            discoverable,
            tag_name,
            url,
            published_at,
            remote_updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12
        )
        ON CONFLICT(uri) DO UPDATE SET
            actor_uri = excluded.actor_uri,
            name = excluded.name,
            description = excluded.description,
            language = excluded.language,
            sensitive = excluded.sensitive,
            discoverable = excluded.discoverable,
            tag_name = excluded.tag_name,
            url = excluded.url,
            published_at = excluded.published_at,
            remote_updated_at = excluded.remote_updated_at,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await
    .map(|_| ())
}

async fn upsert_remote_collection_from_object(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    remote_actor: &RemoteActorProfile,
    object: &serde_json::Value,
) -> Result<Option<RemoteCollectionRow>> {
    let Some(draft) = remote_collection_draft_from_object(remote_actor, object) else {
        return Ok(None);
    };
    upsert_remote_collection_draft(db, &draft).await?;

    let row = remote_collection_row_by_uri(db, &draft.uri).await?;
    if row.is_some() && draft.includes_items {
        replace_remote_collection_items_from_object(db, config, &draft.id, &draft.uri, object)
            .await?;
    }
    Ok(row)
}

async fn replace_remote_collection_items_from_object(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    collection_id: &str,
    collection_uri: &str,
    object: &serde_json::Value,
) -> Result<()> {
    db.prepare("DELETE FROM remote_account_collection_items WHERE collection_id = ?1")
        .bind_refs(&[D1Type::Text(collection_id)])?
        .run()
        .await?;
    for item in featured_collection_items(object) {
        upsert_remote_collection_item_from_object(db, config, collection_id, collection_uri, item)
            .await?;
    }
    Ok(())
}

fn feature_authorization_matches_document(
    document: &serde_json::Value,
    approval_uri: &str,
    collection_uri: &str,
    target_actor_uri: &str,
) -> bool {
    let interacting_object = document
        .get("interactingObject")
        .or_else(|| document.get("interacting_object"))
        .and_then(|value| activitypub_value_id(Some(value)));
    let interaction_target = document
        .get("interactionTarget")
        .or_else(|| document.get("interaction_target"))
        .and_then(|value| activitypub_value_id(Some(value)));
    document.get("type").and_then(serde_json::Value::as_str) == Some("FeatureAuthorization")
        && activitypub_value_id(Some(document)) == Some(approval_uri)
        && interacting_object == Some(collection_uri)
        && interaction_target == Some(target_actor_uri)
}

async fn verify_remote_collection_item_approval(
    config: &cfwdon_core::AppConfig,
    collection_uri: &str,
    target_actor_uri: &str,
    feature_authorization: Option<&str>,
) -> Result<(&'static str, bool)> {
    if local_username_from_actor_uri(config, target_actor_uri).is_some() {
        return Ok(("accepted", false));
    }
    let Some(approval_uri) = feature_authorization else {
        return Ok(("pending", false));
    };
    let document = match fetch_remote_activitypub_document(approval_uri).await {
        Ok(document) => document,
        Err(_) => return Ok(("pending", false)),
    };
    if feature_authorization_matches_document(
        &document,
        approval_uri,
        collection_uri,
        target_actor_uri,
    ) {
        Ok(("accepted", true))
    } else {
        Ok(("pending", false))
    }
}

async fn update_remote_collection_item_approval_verification(
    db: &crate::D1Database,
    collection_id: &str,
    target_actor_uri: &str,
    state: &str,
    approval_verified: bool,
) -> Result<()> {
    let bindings = [
        D1Type::Text(state),
        D1Type::Integer(if approval_verified { 1 } else { 0 }),
        D1Type::Text(collection_id),
        D1Type::Text(target_actor_uri),
    ];
    db.prepare(
        "UPDATE remote_account_collection_items
         SET state = ?1,
             approval_last_verified_at = CASE WHEN ?2 = 1 THEN CURRENT_TIMESTAMP ELSE NULL END,
             updated_at = CURRENT_TIMESTAMP
         WHERE collection_id = ?3
           AND target_actor_uri = ?4",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn revalidate_remote_collection_item_approvals(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    collection: &RemoteCollectionRow,
) -> Result<()> {
    let items =
        list_remote_collection_items_due_for_approval_revalidation(db, &collection.id).await?;
    for item in items {
        if local_username_from_actor_uri(config, &item.target_actor_uri).is_some() {
            continue;
        }
        let Some(approval_uri) = item.feature_authorization.as_deref() else {
            continue;
        };
        let document = match fetch_remote_activitypub_document(approval_uri).await {
            Ok(document) => document,
            Err(_) => continue,
        };
        let verified = feature_authorization_matches_document(
            &document,
            approval_uri,
            &collection.uri,
            &item.target_actor_uri,
        );
        update_remote_collection_item_approval_verification(
            db,
            &collection.id,
            &item.target_actor_uri,
            if verified { "accepted" } else { "pending" },
            verified,
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn revalidate_stale_remote_collection_item_approvals(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    limit: i32,
) -> Result<u32> {
    let items = list_stale_remote_collection_items_for_approval_revalidation(db, limit).await?;
    let mut revalidated = 0;
    for item in items {
        if local_username_from_actor_uri(config, &item.target_actor_uri).is_some() {
            continue;
        }
        let document = match fetch_remote_activitypub_document(&item.feature_authorization).await {
            Ok(document) => document,
            Err(_) => continue,
        };
        let verified = feature_authorization_matches_document(
            &document,
            &item.feature_authorization,
            &item.collection_uri,
            &item.target_actor_uri,
        );
        update_remote_collection_item_approval_verification(
            db,
            &item.collection_id,
            &item.target_actor_uri,
            if verified { "accepted" } else { "pending" },
            verified,
        )
        .await?;
        revalidated += 1;
    }
    Ok(revalidated)
}

async fn upsert_remote_collection_item_from_object(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    collection_id: &str,
    collection_uri: &str,
    object: &serde_json::Value,
) -> Result<()> {
    if object.get("type").and_then(serde_json::Value::as_str) != Some("FeaturedItem") {
        return Ok(());
    }
    let Some(target_actor_uri) = object
        .get("featuredObject")
        .and_then(|value| activitypub_value_id(Some(value)))
    else {
        return Ok(());
    };
    let item_uri = activitypub_value_id(Some(object));
    let item_id = item_uri.map_or_else(
        || remote_account_rest_id(&format!("{collection_id}:{target_actor_uri}")),
        remote_account_rest_id,
    );
    let feature_authorization = object
        .get("featureAuthorization")
        .and_then(serde_json::Value::as_str);
    let (state, approval_verified) = verify_remote_collection_item_approval(
        config,
        collection_uri,
        target_actor_uri,
        feature_authorization,
    )
    .await?;
    let published = object.get("published").and_then(serde_json::Value::as_str);
    let bindings = [
        D1Type::Text(item_id.as_str()),
        D1Type::Text(collection_id),
        item_uri.map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(target_actor_uri),
        D1Type::Text(state),
        feature_authorization.map_or(D1Type::Null, D1Type::Text),
        published.map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(if approval_verified { 1 } else { 0 }),
    ];
    db.prepare(
        "INSERT INTO remote_account_collection_items (
            id,
            collection_id,
            uri,
            target_actor_uri,
            state,
            feature_authorization,
            published_at,
            approval_last_verified_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, CASE WHEN ?8 = 1 THEN CURRENT_TIMESTAMP ELSE NULL END
        )
        ON CONFLICT(collection_id, target_actor_uri) DO UPDATE SET
            uri = excluded.uri,
            state = excluded.state,
            feature_authorization = excluded.feature_authorization,
            published_at = excluded.published_at,
            approval_last_verified_at = CASE
                WHEN ?8 = 1 THEN CURRENT_TIMESTAMP
                ELSE remote_account_collection_items.approval_last_verified_at
            END,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn delete_remote_collection_by_uri(
    db: &crate::D1Database,
    actor_uri: &str,
    collection_uri: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(actor_uri), D1Type::Text(collection_uri)];
    db.prepare(
        "DELETE FROM remote_account_collections
         WHERE actor_uri = ?1
           AND uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn delete_remote_collection_item_by_object(
    db: &crate::D1Database,
    collection_id: &str,
    object: &serde_json::Value,
) -> Result<()> {
    let item_uri = activitypub_value_id(Some(object));
    let target_actor_uri = object
        .get("featuredObject")
        .and_then(|value| activitypub_value_id(Some(value)));
    let bindings = [
        D1Type::Text(collection_id),
        item_uri.map_or(D1Type::Null, D1Type::Text),
        target_actor_uri.map_or(D1Type::Null, D1Type::Text),
    ];
    db.prepare(
        "DELETE FROM remote_account_collection_items
         WHERE collection_id = ?1
           AND (
             (?2 IS NOT NULL AND uri = ?2)
             OR (?3 IS NOT NULL AND target_actor_uri = ?3)
           )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn revoke_remote_collection_item(
    db: &crate::D1Database,
    collection_id: &str,
    item_id: &str,
) -> Result<bool> {
    if remote_collection_item_by_id(db, collection_id, item_id)
        .await?
        .is_none()
    {
        return Ok(false);
    }
    db.prepare(
        "UPDATE remote_account_collection_items
         SET state = 'revoked',
             updated_at = CURRENT_TIMESTAMP
         WHERE collection_id = ?1
           AND id = ?2",
    )
    .bind_refs(&[D1Type::Text(collection_id), D1Type::Text(item_id)])?
    .run()
    .await?;
    Ok(true)
}

fn build_delete_feature_authorization_activity(
    config: &cfwdon_core::AppConfig,
    requester: &cfwdon_domain::LocalAccount,
    collection: &RemoteCollectionRow,
    item: &RemoteCollectionItemRow,
) -> serde_json::Value {
    let feature_authorization = item
        .feature_authorization
        .clone()
        .unwrap_or_else(|| format!("{}/feature_authorization", item.id));
    let actor = actor_url(config, requester.username());
    serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{feature_authorization}#delete"),
        "type": "Delete",
        "actor": actor,
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "object": {
            "id": feature_authorization,
            "type": "FeatureAuthorization",
            "interactingObject": collection.uri,
            "interactionTarget": actor,
        },
    })
}

async fn enqueue_delete_feature_authorization_activity(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    requester: &cfwdon_domain::LocalAccount,
    collection: &RemoteCollectionRow,
    item: &RemoteCollectionItemRow,
) -> Result<()> {
    let payload = build_delete_feature_authorization_activity(config, requester, collection, item);
    let payload_json = serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize delete feature authorization activity: {error}"
        ))
    })?;
    let _ = queue_remote_actor_activity(db, requester.id(), &collection.actor_uri, &payload_json)
        .await?;
    let follower_inboxes = list_follower_delivery_targets(db, requester.id()).await?;
    if !follower_inboxes.is_empty() {
        enqueue_targeted_outbox_activity(
            db,
            requester.id(),
            None,
            &payload_json,
            &follower_inboxes,
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn handle_inbox_collection_add(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    let Some(target) = activitypub_value_id(activity.get("target")) else {
        return Ok(());
    };
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(());
    };
    upsert_remote_actor(db, remote_actor).await?;
    if is_remote_actor_collections_target(&remote_actor.actor_uri, target) {
        let _ = upsert_remote_collection_from_object(db, config, remote_actor, object).await?;
        return Ok(());
    }
    let Some(collection) = remote_collection_row_by_uri(db, target).await? else {
        return Ok(());
    };
    if collection.actor_uri != remote_actor.actor_uri {
        return Ok(());
    }
    upsert_remote_collection_item_from_object(db, config, &collection.id, &collection.uri, object)
        .await
}

pub(crate) async fn handle_inbox_collection_update(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<bool> {
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(false);
    };
    if object.get("type").and_then(serde_json::Value::as_str) != Some("FeaturedCollection") {
        return Ok(false);
    }
    upsert_remote_actor(db, remote_actor).await?;
    let _ = upsert_remote_collection_from_object(db, config, remote_actor, object).await?;
    Ok(true)
}

pub(crate) async fn handle_inbox_collection_remove(
    db: &crate::D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    let Some(target) = activitypub_value_id(activity.get("target")) else {
        return Ok(());
    };
    let Some(object) = activity.get("object") else {
        return Ok(());
    };
    if is_remote_actor_collections_target(&remote_actor.actor_uri, target) {
        if let Some(collection_uri) = activitypub_value_id(Some(object)) {
            delete_remote_collection_by_uri(db, &remote_actor.actor_uri, collection_uri).await?;
        }
        return Ok(());
    }
    let Some(collection) = remote_collection_row_by_uri(db, target).await? else {
        return Ok(());
    };
    if collection.actor_uri != remote_actor.actor_uri {
        return Ok(());
    }
    delete_remote_collection_item_by_object(db, &collection.id, object).await
}

fn feature_response_object_uri(activity: &serde_json::Value) -> Option<&str> {
    activitypub_value_id(activity.get("object"))
}

fn feature_response_result_uri(activity: &serde_json::Value) -> Option<&str> {
    match activity.get("result") {
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .find_map(|value| activitypub_value_id(Some(value))),
        value => activitypub_value_id(value),
    }
}

pub(crate) async fn handle_inbox_collection_feature_accept(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<bool> {
    let Some(feature_request_uri) = feature_response_object_uri(activity) else {
        return Ok(false);
    };
    let Some(approval_uri) = feature_response_result_uri(activity) else {
        return Ok(false);
    };
    let Some((collection, item)) =
        collection_item_by_feature_request_uri(db, feature_request_uri).await?
    else {
        return Ok(false);
    };
    if item.target_account_ref != remote_account_rest_id(&remote_actor.actor_uri) {
        return Ok(false);
    }
    let Some(owner) = find_account_by_id(db, &collection.account_id).await? else {
        return Ok(false);
    };
    let Some(item) = update_collection_item_feature_state(
        db,
        &collection.id,
        &item.id,
        "accepted",
        Some(approval_uri),
    )
    .await?
    else {
        return Ok(true);
    };
    enqueue_collection_item_add_activity(db, config, &owner, &collection.id, &item).await?;
    Ok(true)
}

pub(crate) async fn handle_inbox_collection_feature_reject(
    db: &crate::D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<bool> {
    let Some(feature_request_uri) = feature_response_object_uri(activity) else {
        return Ok(false);
    };
    let Some((collection, item)) =
        collection_item_by_feature_request_uri(db, feature_request_uri).await?
    else {
        return Ok(false);
    };
    if item.target_account_ref != remote_account_rest_id(&remote_actor.actor_uri) {
        return Ok(false);
    }
    let _ = update_collection_item_feature_state(db, &collection.id, &item.id, "rejected", None)
        .await?;
    Ok(true)
}

pub(crate) async fn handle_inbox_collection_feature_authorization_delete(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<bool> {
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(false);
    };
    if object.get("type").and_then(serde_json::Value::as_str) != Some("FeatureAuthorization") {
        return Ok(false);
    }
    let Some(collection_uri) = object
        .get("interactingObject")
        .and_then(|value| activitypub_value_id(Some(value)))
    else {
        return Ok(false);
    };
    let Some(featured_actor_uri) = object
        .get("interactionTarget")
        .and_then(|value| activitypub_value_id(Some(value)))
    else {
        return Ok(false);
    };
    if featured_actor_uri != remote_actor.actor_uri {
        return Ok(false);
    }
    let Some(collection_id) = local_collection_id_from_uri(config, collection_uri) else {
        return Ok(false);
    };
    let Some(collection) = collection_row_by_id(db, &collection_id).await? else {
        return Ok(false);
    };
    let target_ref = remote_account_rest_id(featured_actor_uri);
    let row = db
        .prepare(
            "SELECT id
             FROM account_collection_items
             WHERE collection_id = ?1
               AND target_account_ref = ?2
             LIMIT 1",
        )
        .bind_refs(&[
            D1Type::Text(collection.id.as_str()),
            D1Type::Text(&target_ref),
        ])?
        .first::<serde_json::Value>(None)
        .await?;
    let Some(item_id) = row
        .as_ref()
        .and_then(|value| value.get("id"))
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(true);
    };
    let _ =
        update_collection_item_feature_state(db, &collection.id, item_id, "revoked", None).await?;
    Ok(true)
}

fn merge_collection_notification_policy_action(
    current: CollectionNotificationPolicyAction,
    policy_value: &str,
    condition_matches: bool,
) -> CollectionNotificationPolicyAction {
    if !condition_matches || current == CollectionNotificationPolicyAction::Drop {
        return current;
    }
    match policy_value {
        "drop" => CollectionNotificationPolicyAction::Drop,
        "filter" => CollectionNotificationPolicyAction::Filter,
        _ => current,
    }
}

async fn accepted_follow_exists(
    db: &crate::D1Database,
    follower_account_id: &str,
    target_actor_uri: &str,
) -> Result<bool> {
    let bindings = [
        D1Type::Text(follower_account_id),
        D1Type::Text(target_actor_uri),
    ];
    let row = db
        .prepare(
            "SELECT 1 AS found
             FROM follows
             WHERE follower_account_id = ?1
               AND target_actor_uri = ?2
               AND state = 'accepted'
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<CountRow>(None)
        .await?;
    Ok(row.is_some())
}

async fn recent_accepted_follow_exists(
    db: &crate::D1Database,
    follower_account_id: &str,
    target_actor_uri: &str,
    threshold: &str,
) -> Result<bool> {
    let bindings = [
        D1Type::Text(follower_account_id),
        D1Type::Text(target_actor_uri),
        D1Type::Text(threshold),
    ];
    let row = db
        .prepare(
            "SELECT 1 AS found
             FROM follows
             WHERE follower_account_id = ?1
               AND target_actor_uri = ?2
               AND state = 'accepted'
               AND datetime(replace(replace(created_at, 'T', ' '), 'Z', '')) > datetime(CURRENT_TIMESTAMP, ?3)
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<CountRow>(None)
        .await?;
    Ok(row.is_some())
}

async fn timestamp_is_after_current_timestamp_modifier(
    db: &crate::D1Database,
    timestamp: &str,
    modifier: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(timestamp), D1Type::Text(modifier)];
    let row = db
        .prepare(
            "SELECT 1 AS found
             WHERE datetime(replace(replace(?1, 'T', ' '), 'Z', '')) > datetime(CURRENT_TIMESTAMP, ?2)",
        )
        .bind_refs(bindings.iter())?
        .first::<CountRow>(None)
        .await?;
    Ok(row.is_some())
}

async fn collection_notification_filtered(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    recipient: &cfwdon_domain::LocalAccount,
    sender: &cfwdon_domain::LocalAccount,
    notification_type: &str,
) -> Result<Option<bool>> {
    let sender_actor_uri = actor_url(config, sender.username());
    if is_blocking_actor(db, recipient.id(), &sender_actor_uri).await?
        || muted_notifications_for_actor(db, recipient.id(), &sender_actor_uri).await?
    {
        return Ok(None);
    }
    if notification_type != "added_to_collection" {
        return Ok(Some(false));
    }

    let policy = load_notification_policy_row(db, recipient.id()).await?;
    let recipient_follows_sender =
        accepted_follow_exists(db, recipient.id(), &sender_actor_uri).await?;
    let recipient_actor_uri = actor_url(config, recipient.username());
    let sender_follows_recipient =
        accepted_follow_exists(db, sender.id(), &recipient_actor_uri).await?;
    let sender_is_new_follower =
        recent_accepted_follow_exists(db, sender.id(), &recipient_actor_uri, "-3 days").await?;
    let sender_is_new_account =
        timestamp_is_after_current_timestamp_modifier(db, sender.created_at(), "-30 days").await?;

    let mut action = CollectionNotificationPolicyAction::Deliver;
    action = merge_collection_notification_policy_action(
        action,
        &policy.for_not_following,
        !recipient_follows_sender,
    );
    action = merge_collection_notification_policy_action(
        action,
        &policy.for_not_followers,
        !sender_follows_recipient || sender_is_new_follower,
    );
    action = merge_collection_notification_policy_action(
        action,
        &policy.for_new_accounts,
        sender_is_new_account && !recipient_follows_sender,
    );

    Ok(match action {
        CollectionNotificationPolicyAction::Deliver => Some(false),
        CollectionNotificationPolicyAction::Filter => Some(true),
        CollectionNotificationPolicyAction::Drop => None,
    })
}

async fn insert_collection_notification(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    recipient: &cfwdon_domain::LocalAccount,
    sender: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    collection_item_id: Option<&str>,
    notification_type: &str,
) -> Result<()> {
    let Some(filtered) =
        collection_notification_filtered(db, config, recipient, sender, notification_type).await?
    else {
        return Ok(());
    };
    let notification_id = generate_entity_id(16)?;
    let collection_item_key = collection_item_id.unwrap_or("");
    let bindings = [
        D1Type::Text(notification_id.as_str()),
        D1Type::Text(recipient.id()),
        D1Type::Text(sender.id()),
        D1Type::Text(collection_id),
        collection_item_id.map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(collection_item_key),
        D1Type::Text(notification_type),
        D1Type::Integer(if filtered { 1 } else { 0 }),
    ];
    db.prepare(
        "INSERT INTO collection_notifications (
            id,
            account_id,
            from_account_id,
            collection_id,
            collection_item_id,
            collection_item_key,
            notification_type,
            filtered
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            ?8
        )
        ON CONFLICT(
            account_id,
            notification_type,
            collection_id,
            collection_item_key
        ) DO UPDATE SET
            filtered = excluded.filtered,
            created_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn insert_added_to_collection_notification(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    target: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item_id: &str,
) -> Result<()> {
    if owner.id() == target.id() {
        return Ok(());
    }
    insert_collection_notification(
        db,
        config,
        target,
        owner,
        collection_id,
        Some(item_id),
        "added_to_collection",
    )
    .await
}

async fn insert_collection_update_notifications(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
) -> Result<()> {
    for item in list_collection_items(db, collection_id, false).await? {
        let Some(AccountReference::Local(target)) =
            resolve_account_reference(db, &item.target_account_ref).await?
        else {
            continue;
        };
        if target.id() == owner.id() {
            continue;
        }
        insert_collection_notification(
            db,
            config,
            &target,
            owner,
            collection_id,
            None,
            "collection_update",
        )
        .await?;
    }
    Ok(())
}

async fn list_collection_notifications_for_account(
    db: &crate::D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<CollectionNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT id,
                    from_account_id,
                    collection_id,
                    collection_item_id,
                    notification_type,
                    filtered,
                    created_at
             FROM collection_notifications
             WHERE account_id = ?1
             ORDER BY created_at DESC, id DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    result.results::<CollectionNotificationRow>()
}

pub(crate) async fn collect_collection_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &cfwdon_domain::LocalAccount,
    query: &crate::NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    for notification in
        list_collection_notifications_for_account(db, viewer.id(), per_type_limit).await?
    {
        if !notification_type_allowed(query, &notification.notification_type) {
            continue;
        }
        if notification.filtered != 0 && query.account_id.is_none() {
            continue;
        }
        let Some(owner) = find_account_by_id(db, &notification.from_account_id).await? else {
            continue;
        };
        if muted_notifications_for_actor(db, viewer.id(), &actor_url(config, owner.username()))
            .await?
            || !notification_account_matches_filter(query.account_id.as_deref(), owner.id(), None)
        {
            continue;
        }
        let Some(collection) = collection_row_by_id(db, &notification.collection_id).await? else {
            continue;
        };
        let items = list_collection_items(db, &collection.id, false)
            .await?
            .iter()
            .map(collection_item_document)
            .collect::<Vec<_>>();
        let created_at = timestamp_to_mastodon_iso8601(&notification.created_at);
        let mut value = serde_json::to_value(MastodonNotificationResponse {
            id: notification.id.clone(),
            notification_type: notification.notification_type.clone(),
            group_key: format!(
                "{}-{}",
                notification.notification_type, notification.collection_id
            ),
            created_at: created_at.clone(),
            account: MastodonAccountResponse::from_account(&owner, config),
            status: None,
            report: None,
        })?;
        value["collection"] = collection_document(config, &owner, &collection, items);
        if let Some(item_id) = notification.collection_item_id.as_deref() {
            value["collection_item_id"] = serde_json::json!(item_id);
        }
        entries.push(NotificationEntry {
            id: notification.id,
            created_at,
            value,
        });
    }
    Ok(())
}

async fn insert_collection(
    db: &crate::D1Database,
    account_id: &str,
    request: &CollectionRequest,
) -> Result<CollectionRow> {
    let collection_id = generate_entity_id(16)?;
    let bindings = [
        D1Type::Text(collection_id.as_str()),
        D1Type::Text(account_id),
        D1Type::Text(request.name.as_deref().unwrap_or_default()),
        D1Type::Text(request.description.as_deref().unwrap_or_default()),
        request
            .language
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(if request.sensitive.unwrap_or(false) {
            1
        } else {
            0
        }),
        D1Type::Integer(if request.discoverable.unwrap_or(true) {
            1
        } else {
            0
        }),
        request
            .tag_name
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
    ];
    db.prepare(
        "INSERT INTO account_collections (
            id,
            account_id,
            name,
            description,
            language,
            sensitive,
            discoverable,
            tag_name
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            ?8
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    collection_row_by_id(db, &collection_id)
        .await?
        .ok_or_else(|| worker::Error::RustError("failed to reload created collection".to_owned()))
}

async fn update_collection(
    db: &crate::D1Database,
    collection_id: &str,
    request: &CollectionRequest,
) -> Result<Option<CollectionRow>> {
    let existing = collection_row_by_id(db, collection_id).await?;
    let Some(existing) = existing else {
        return Ok(None);
    };
    let bindings = [
        D1Type::Text(request.name.as_deref().unwrap_or(&existing.name)),
        D1Type::Text(
            request
                .description
                .as_deref()
                .unwrap_or(&existing.description),
        ),
        request.language.as_deref().map_or_else(
            || {
                existing
                    .language
                    .as_deref()
                    .map_or(D1Type::Null, D1Type::Text)
            },
            D1Type::Text,
        ),
        D1Type::Integer(if request.sensitive.unwrap_or(existing.sensitive != 0) {
            1
        } else {
            0
        }),
        D1Type::Integer(
            if request.discoverable.unwrap_or(existing.discoverable != 0) {
                1
            } else {
                0
            },
        ),
        request.tag_name.as_deref().map_or_else(
            || {
                existing
                    .tag_name
                    .as_deref()
                    .map_or(D1Type::Null, D1Type::Text)
            },
            D1Type::Text,
        ),
        D1Type::Text(collection_id),
    ];
    db.prepare(
        "UPDATE account_collections
         SET name = ?1,
             description = ?2,
             language = ?3,
             sensitive = ?4,
             discoverable = ?5,
             tag_name = ?6,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?7",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    collection_row_by_id(db, collection_id).await
}

async fn delete_collection(db: &crate::D1Database, collection_id: &str) -> Result<bool> {
    if collection_row_by_id(db, collection_id).await?.is_none() {
        return Ok(false);
    }
    let collection_id = D1Type::Text(collection_id);
    db.prepare("DELETE FROM account_collections WHERE id = ?1")
        .bind_refs(&collection_id)?
        .run()
        .await?;
    Ok(true)
}

async fn insert_collection_item(
    db: &crate::D1Database,
    collection_id: &str,
    target: &AccountReference,
) -> Result<CollectionItemRow> {
    let item_id = generate_entity_id(16)?;
    let (target_ref, state) = match target {
        AccountReference::Local(account) => (account.id().to_owned(), "accepted"),
        AccountReference::Remote(actor) => (remote_account_rest_id(&actor.actor_uri), "pending"),
    };
    let bindings = [
        D1Type::Text(item_id.as_str()),
        D1Type::Text(collection_id),
        D1Type::Text(target_ref.as_str()),
        D1Type::Text(state),
    ];
    db.prepare(
        "INSERT INTO account_collection_items (
            id,
            collection_id,
            target_account_ref,
            state
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4
        )
        ON CONFLICT(collection_id, target_account_ref) DO UPDATE SET
            state = excluded.state,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    db.prepare(
        "SELECT id, target_account_ref, state, activity_uri, feature_authorization, created_at
         FROM account_collection_items
         WHERE collection_id = ?1
           AND target_account_ref = ?2",
    )
    .bind_refs(&[
        D1Type::Text(collection_id),
        D1Type::Text(target_ref.as_str()),
    ])?
    .first::<CollectionItemRow>(None)
    .await?
    .ok_or_else(|| worker::Error::RustError("failed to reload collection item".to_owned()))
}

async fn collection_item_by_id(
    db: &crate::D1Database,
    collection_id: &str,
    item_id: &str,
) -> Result<Option<CollectionItemRow>> {
    db.prepare(
        "SELECT id, target_account_ref, state, activity_uri, feature_authorization, created_at
         FROM account_collection_items
         WHERE collection_id = ?1
           AND id = ?2
         LIMIT 1",
    )
    .bind_refs(&[D1Type::Text(collection_id), D1Type::Text(item_id)])?
    .first::<CollectionItemRow>(None)
    .await
}

async fn collection_item_by_feature_request_uri(
    db: &crate::D1Database,
    activity_uri: &str,
) -> Result<Option<(CollectionRow, CollectionItemRow)>> {
    let activity_uri_binding = D1Type::Text(activity_uri);
    let row = db
        .prepare(
            "SELECT c.id AS collection_id,
                    i.id AS item_id
             FROM account_collection_items i
             JOIN account_collections c
               ON c.id = i.collection_id
             WHERE i.activity_uri = ?1
             LIMIT 1",
        )
        .bind_refs(&activity_uri_binding)?
        .first::<serde_json::Value>(None)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let Some(collection_id) = row.get("collection_id").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let Some(item_id) = row.get("item_id").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let Some(collection) = collection_row_by_id(db, collection_id).await? else {
        return Ok(None);
    };
    let Some(item) = collection_item_by_id(db, collection_id, item_id).await? else {
        return Ok(None);
    };
    Ok(Some((collection, item)))
}

async fn update_collection_item_feature_request_uri(
    db: &crate::D1Database,
    collection_id: &str,
    item_id: &str,
    activity_uri: &str,
) -> Result<()> {
    db.prepare(
        "UPDATE account_collection_items
         SET activity_uri = ?3,
             updated_at = CURRENT_TIMESTAMP
         WHERE collection_id = ?1
           AND id = ?2",
    )
    .bind_refs(&[
        D1Type::Text(collection_id),
        D1Type::Text(item_id),
        D1Type::Text(activity_uri),
    ])?
    .run()
    .await?;
    Ok(())
}

async fn update_collection_item_feature_state(
    db: &crate::D1Database,
    collection_id: &str,
    item_id: &str,
    state: &str,
    feature_authorization: Option<&str>,
) -> Result<Option<CollectionItemRow>> {
    let bindings = [
        D1Type::Text(state),
        feature_authorization.map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(collection_id),
        D1Type::Text(item_id),
    ];
    db.prepare(
        "UPDATE account_collection_items
         SET state = ?1,
             feature_authorization = COALESCE(?2, feature_authorization),
             updated_at = CURRENT_TIMESTAMP
         WHERE collection_id = ?3
           AND id = ?4",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    collection_item_by_id(db, collection_id, item_id).await
}

async fn delete_collection_item(
    db: &crate::D1Database,
    collection_id: &str,
    item_id: &str,
) -> Result<bool> {
    if collection_item_by_id(db, collection_id, item_id)
        .await?
        .is_none()
    {
        return Ok(false);
    }
    db.prepare(
        "DELETE FROM account_collection_items
         WHERE collection_id = ?1
           AND id = ?2",
    )
    .bind_refs(&[D1Type::Text(collection_id), D1Type::Text(item_id)])?
    .run()
    .await?;
    Ok(true)
}

async fn revoke_collection_item(
    db: &crate::D1Database,
    collection_id: &str,
    item_id: &str,
) -> Result<bool> {
    if collection_item_by_id(db, collection_id, item_id)
        .await?
        .is_none()
    {
        return Ok(false);
    }
    db.prepare(
        "UPDATE account_collection_items
         SET state = 'revoked',
             updated_at = CURRENT_TIMESTAMP
         WHERE collection_id = ?1
           AND id = ?2",
    )
    .bind_refs(&[D1Type::Text(collection_id), D1Type::Text(item_id)])?
    .run()
    .await?;
    Ok(true)
}

fn can_revoke_collection_item(
    requester: &cfwdon_domain::LocalAccount,
    item: &CollectionItemRow,
) -> bool {
    item.target_account_ref == requester.id()
}

fn route_param(ctx: &RouteContext<()>, name: &str) -> Result<String> {
    ctx.param(name)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError(format!("missing {name} route parameter")))
}

fn build_collection_offset_link(url: &url::Url, limit: u32, offset: u32, rel: &str) -> String {
    let mut url = url.clone();
    let query_pairs = url
        .query_pairs()
        .filter(|(key, _)| key != "limit" && key != "offset")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    url.set_query(None);
    {
        let mut serializer = url.query_pairs_mut();
        for (key, value) in query_pairs {
            serializer.append_pair(&key, &value);
        }
        serializer.append_pair("limit", &limit.to_string());
        serializer.append_pair("offset", &offset.to_string());
    }
    format!("<{}>; rel=\"{rel}\"", url.as_str())
}

fn build_collection_offset_link_header_for_url(
    url: &url::Url,
    limit: u32,
    offset: u32,
    page_size: usize,
    total_count: u64,
) -> Option<String> {
    let mut links = Vec::new();
    if (offset as u64).saturating_add(page_size as u64) < total_count {
        links.push(build_collection_offset_link(
            url,
            limit,
            offset.saturating_add(limit),
            "next",
        ));
    }
    if offset > 0 {
        links.push(build_collection_offset_link(
            url,
            limit,
            offset.saturating_sub(limit),
            "prev",
        ));
    }
    (!links.is_empty()).then(|| links.join(", "))
}

fn build_collection_offset_link_header(
    req: &Request,
    limit: u32,
    offset: u32,
    page_size: usize,
    total_count: u64,
) -> Result<Option<String>> {
    Ok(build_collection_offset_link_header_for_url(
        &req.url()?,
        limit,
        offset,
        page_size,
        total_count,
    ))
}

pub(crate) async fn alpha_account_collections_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: CollectionsQuery = req.query().unwrap_or_default();
    let limit = query
        .limit
        .unwrap_or(DEFAULT_COLLECTIONS_LIMIT)
        .clamp(1, MAX_COLLECTIONS_LIMIT);
    let offset = query.offset.unwrap_or(0);
    let account_id = route_param(&ctx, "account_id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match optional_collection_viewer(&req, &db, &config).await? {
        Ok(viewer) => viewer,
        Err(response) => return Ok(response),
    };

    let owner = match resolve_account_reference(&db, &account_id).await? {
        Some(AccountReference::Local(account)) => account,
        Some(AccountReference::Remote(_)) => {
            let Some(AccountReference::Remote(owner)) =
                resolve_account_reference(&db, &account_id).await?
            else {
                return Response::error("account not found", 404);
            };
            if viewer
                .account
                .as_ref()
                .is_some_and(|viewer| viewer.id() == account_id)
                || (if let Some(viewer) = viewer.account.as_ref() {
                    is_blocking_actor(&db, viewer.id(), &owner.actor_uri).await?
                } else {
                    false
                })
            {
                return Response::from_json(&collection_list_document(Vec::new()));
            }
            let rows =
                list_remote_collection_rows_for_actor(&db, &owner.actor_uri, offset, limit).await?;
            let total_count = count_remote_collection_rows_for_actor(&db, &owner.actor_uri).await?;
            let mut response = Vec::new();
            for row in rows.iter() {
                revalidate_remote_collection_item_approvals(&db, &config, row).await?;
                let items = list_remote_collection_items(&db, &row.id, false).await?;
                let mut item_documents = Vec::new();
                for item in &items {
                    item_documents.push(remote_collection_item_document(&db, &config, item).await?);
                }
                response.push(remote_collection_document(
                    &config,
                    &owner,
                    row,
                    item_documents,
                ));
            }
            let mut builder = Response::from_json(&collection_list_document(response))?;
            if let Some(link_header) =
                build_collection_offset_link_header(&req, limit, offset, rows.len(), total_count)?
            {
                builder.headers_mut().set("Link", &link_header)?;
            }
            return Ok(builder);
        }
        None => return Response::error("account not found", 404),
    };
    let include_private = is_owner(viewer.account.as_ref(), owner.id());
    let collections_hidden =
        account_blocks_viewer(&db, &config, &owner, viewer.account.as_ref()).await?;
    let (rows, total_count) = if collections_hidden {
        (Vec::new(), 0)
    } else {
        (
            list_collection_rows_for_account(&db, owner.id(), include_private, offset, limit)
                .await?,
            count_collection_rows_for_account(&db, owner.id(), include_private).await?,
        )
    };
    let mut response = Vec::new();
    for row in rows.iter() {
        let include_pending = include_private;
        let items = list_collection_items(&db, &row.id, include_pending)
            .await?
            .iter()
            .map(collection_item_document)
            .collect::<Vec<_>>();
        response.push(collection_document(&config, &owner, row, items));
    }
    let mut builder = Response::from_json(&collection_list_document(response))?;
    if let Some(link_header) =
        build_collection_offset_link_header(&req, limit, offset, rows.len(), total_count)?
    {
        builder.headers_mut().set("Link", &link_header)?;
    }
    Ok(builder)
}

pub(crate) async fn alpha_account_in_collections_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: CollectionsQuery = req.query().unwrap_or_default();
    let limit = query
        .limit
        .unwrap_or(DEFAULT_COLLECTIONS_LIMIT)
        .clamp(1, MAX_COLLECTIONS_LIMIT);
    let offset = query.offset.unwrap_or(0);
    let account_id = route_param(&ctx, "account_id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_collection_reader(&req, &db, &config).await? {
        Ok(viewer) => viewer,
        Err(response) => return Ok(response),
    };
    let target_account_id = match resolve_account_reference(&db, &account_id).await? {
        Some(AccountReference::Local(account)) if account.id() == viewer.id() => {
            account.id().to_owned()
        }
        Some(_) => return action_not_allowed_response(),
        None => return Response::error("account not found", 404),
    };

    let target_actor_uri = actor_url(&config, viewer.username());
    let local_total_count = count_in_collection_rows(&db, &target_account_id).await?;
    let remote_total_count = count_remote_in_collection_rows(&db, &target_actor_uri).await?;
    let total_count = local_total_count.saturating_add(remote_total_count);
    let page_window = offset.saturating_add(limit);
    let local_rows = list_local_in_collection_rows(&db, &target_account_id, page_window).await?;
    let remote_rows = list_remote_in_collection_rows(&db, &target_actor_uri, page_window).await?;
    let mut entries = local_rows
        .into_iter()
        .map(InCollectionPageEntry::Local)
        .chain(remote_rows.into_iter().map(InCollectionPageEntry::Remote))
        .collect::<Vec<_>>();
    sort_in_collection_page_entries(&mut entries);

    let page_entries = entries
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect::<Vec<_>>();
    let page_size = page_entries.len();
    let mut response = Vec::new();
    for entry in page_entries {
        match entry {
            InCollectionPageEntry::Local(row) => {
                let Some(owner) = find_account_by_id(&db, &row.account_id).await? else {
                    continue;
                };
                let include_pending = viewer.id() == owner.id();
                let items = list_collection_items(&db, &row.id, include_pending)
                    .await?
                    .iter()
                    .map(collection_item_document)
                    .collect::<Vec<_>>();
                response.push(collection_document(&config, &owner, &row, items));
            }
            InCollectionPageEntry::Remote(row) => {
                let Some(owner) = find_remote_actor_by_actor_uri(&db, &row.actor_uri).await? else {
                    continue;
                };
                revalidate_remote_collection_item_approvals(&db, &config, &row).await?;
                let items = list_remote_collection_items(&db, &row.id, true).await?;
                let mut item_documents = Vec::new();
                for item in &items {
                    item_documents.push(remote_collection_item_document(&db, &config, item).await?);
                }
                response.push(remote_collection_document(
                    &config,
                    &owner,
                    &row,
                    item_documents,
                ));
            }
        }
    }
    let mut builder = Response::from_json(&collection_list_document(response))?;
    if let Some(link_header) =
        build_collection_offset_link_header(&req, limit, offset, page_size, total_count)?
    {
        builder.headers_mut().set("Link", &link_header)?;
    }
    Ok(builder)
}

pub(crate) async fn alpha_collection_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let collection_id = route_param(&ctx, "id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match optional_collection_viewer(&req, &db, &config).await? {
        Ok(viewer) => viewer,
        Err(response) => return Ok(response),
    };
    let Some(row) = collection_row_by_id(&db, &collection_id).await? else {
        let Some(row) = remote_collection_row_by_id(&db, &collection_id).await? else {
            return Response::error("collection not found", 404);
        };
        let Some(owner) = find_remote_actor_by_actor_uri(&db, &row.actor_uri).await? else {
            return Response::error("collection not found", 404);
        };
        if let Some(viewer) = viewer.account.as_ref()
            && is_blocking_actor(&db, viewer.id(), &owner.actor_uri).await?
        {
            return Response::error("collection not found", 404);
        }
        let document = remote_collection_with_accounts_document(
            &db,
            &config,
            &owner,
            &row,
            false,
            viewer.account.as_ref(),
        )
        .await?;
        return Response::from_json(&document);
    };
    let Some(owner) = find_account_by_id(&db, &row.account_id).await? else {
        return Response::error("collection not found", 404);
    };
    if account_blocks_viewer(&db, &config, &owner, viewer.account.as_ref()).await? {
        return Response::error("collection not found", 404);
    }
    let document = collection_with_accounts_document(
        &db,
        &config,
        &owner,
        &row,
        is_owner(viewer.account.as_ref(), &row.account_id),
        viewer.account.as_ref(),
    )
    .await?;
    Response::from_json(&document)
}

pub(crate) async fn create_alpha_collection_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let owner = match require_collection_writer(&req, &db, &config).await? {
        Ok(account) => account,
        Err(response) => return Ok(response),
    };
    let request = match parse_collection_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    let details = validate_collection_request(&request, true);
    if !details.is_empty() {
        return validation_failed_response(details);
    }

    let mut targets = Vec::new();
    if let Some(account_ids) = request.account_ids.as_ref() {
        for account_id in account_ids.iter().take(MAX_COLLECTION_ITEMS) {
            let Some(target) = resolve_account_reference(&db, account_id).await? else {
                return Response::error("account not found", 404);
            };
            if !account_reference_featureable_by_owner(&db, &config, &owner, &target).await? {
                return action_not_allowed_response();
            }
            targets.push(target);
        }
    }

    let row = insert_collection(&db, owner.id(), &request).await?;
    for target in targets {
        let item = insert_collection_item(&db, &row.id, &target).await?;
        match target {
            AccountReference::Local(target) => {
                insert_added_to_collection_notification(
                    &db, &config, &owner, &target, &row.id, &item.id,
                )
                .await?;
            }
            AccountReference::Remote(actor) => {
                enqueue_collection_feature_request_activity(
                    &db,
                    &config,
                    &owner,
                    &row.id,
                    &item,
                    &actor.actor_uri,
                )
                .await?;
            }
        }
    }
    let row = collection_row_by_id(&db, &row.id)
        .await?
        .ok_or_else(|| worker::Error::RustError("failed to reload collection".to_owned()))?;
    enqueue_collection_add_activity(&db, &config, &owner, &row).await?;
    let items = list_collection_items(&db, &row.id, true)
        .await?
        .iter()
        .map(collection_item_document)
        .collect::<Vec<_>>();
    Response::from_json(&collection_response_document(collection_document(
        &config, &owner, &row, items,
    )))
}

pub(crate) async fn update_alpha_collection_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let collection_id = route_param(&ctx, "id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let owner = match require_collection_writer(&req, &db, &config).await? {
        Ok(account) => account,
        Err(response) => return Ok(response),
    };
    let Some(existing) = collection_row_by_id(&db, &collection_id).await? else {
        return Response::error("collection not found", 404);
    };
    if existing.account_id != owner.id() {
        return action_not_allowed_response();
    }
    let request = match parse_collection_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    let details = validate_collection_request(&request, false);
    if !details.is_empty() {
        return validation_failed_response(details);
    }
    let distribute_update = collection_update_requires_activity(&existing, &request);
    let significant_update = collection_update_is_significant(&existing, &request);
    let row = update_collection(&db, &collection_id, &request)
        .await?
        .ok_or_else(|| worker::Error::RustError("updated collection disappeared".to_owned()))?;
    if distribute_update {
        enqueue_collection_update_activity(&db, &config, &owner, &row).await?;
    }
    if significant_update {
        insert_collection_update_notifications(&db, &config, &owner, &row.id).await?;
    }
    let items = list_collection_items(&db, &row.id, true)
        .await?
        .iter()
        .map(collection_item_document)
        .collect::<Vec<_>>();
    Response::from_json(&collection_response_document(collection_document(
        &config, &owner, &row, items,
    )))
}

pub(crate) async fn delete_alpha_collection_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let collection_id = route_param(&ctx, "id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let owner = match require_collection_writer(&req, &db, &config).await? {
        Ok(account) => account,
        Err(response) => return Ok(response),
    };
    let Some(existing) = collection_row_by_id(&db, &collection_id).await? else {
        return Response::error("collection not found", 404);
    };
    if existing.account_id != owner.id() {
        return action_not_allowed_response();
    }
    enqueue_collection_remove_activity(&db, &config, &owner, &existing).await?;
    let _ = delete_collection(&db, &collection_id).await?;
    Ok(Response::empty()?.with_status(200))
}

pub(crate) async fn create_alpha_collection_item_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let collection_id = route_param(&ctx, "collection_id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let owner = match require_collection_writer(&req, &db, &config).await? {
        Ok(account) => account,
        Err(response) => return Ok(response),
    };
    let Some(collection) = collection_row_by_id(&db, &collection_id).await? else {
        return Response::error("collection not found", 404);
    };
    if collection.account_id != owner.id() {
        return action_not_allowed_response();
    }
    let request = match parse_collection_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    let Some(account_id) = request.account_id.clone().or_else(|| {
        request
            .account_ids
            .as_ref()
            .and_then(|ids| ids.first())
            .cloned()
    }) else {
        return Response::from_json(&serde_json::json!({
            "error": "`account_id` parameter is missing",
        }))
        .map(|response| response.with_status(422));
    };
    let Some(target) = resolve_account_reference(&db, &account_id).await? else {
        return Response::error("account not found", 404);
    };
    if !account_reference_featureable_by_owner(&db, &config, &owner, &target).await? {
        return action_not_allowed_response();
    }
    let item = insert_collection_item(&db, &collection_id, &target).await?;
    match target {
        AccountReference::Local(target) => {
            enqueue_collection_item_add_activity(&db, &config, &owner, &collection_id, &item)
                .await?;
            insert_added_to_collection_notification(
                &db,
                &config,
                &owner,
                &target,
                &collection_id,
                &item.id,
            )
            .await?;
        }
        AccountReference::Remote(actor) => {
            enqueue_collection_feature_request_activity(
                &db,
                &config,
                &owner,
                &collection_id,
                &item,
                &actor.actor_uri,
            )
            .await?;
        }
    }
    Response::from_json(&collection_item_response_document(
        collection_item_document(&item),
    ))
}

pub(crate) async fn delete_alpha_collection_item_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let collection_id = route_param(&ctx, "collection_id")?;
    let item_id = route_param(&ctx, "id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let owner = match require_collection_writer(&req, &db, &config).await? {
        Ok(account) => account,
        Err(response) => return Ok(response),
    };
    let Some(collection) = collection_row_by_id(&db, &collection_id).await? else {
        return Response::error("collection not found", 404);
    };
    if collection.account_id != owner.id() {
        return action_not_allowed_response();
    }
    let Some(item) = collection_item_by_id(&db, &collection_id, &item_id).await? else {
        return Response::error("collection item not found", 404);
    };
    enqueue_collection_item_remove_activity(&db, &config, &owner, &collection_id, &item).await?;
    if !delete_collection_item(&db, &collection_id, &item_id).await? {
        return Response::error("collection item not found", 404);
    }
    Ok(Response::empty()?.with_status(200))
}

pub(crate) async fn revoke_alpha_collection_item_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let collection_id = route_param(&ctx, "collection_id")?;
    let item_id = route_param(&ctx, "id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let requester = match require_collection_writer(&req, &db, &config).await? {
        Ok(account) => account,
        Err(response) => return Ok(response),
    };
    let Some(_collection) = collection_row_by_id(&db, &collection_id).await? else {
        let Some(remote_collection) = remote_collection_row_by_id(&db, &collection_id).await?
        else {
            return Response::error("collection not found", 404);
        };
        let Some(item) = remote_collection_item_by_id(&db, &collection_id, &item_id).await? else {
            return Response::error("collection item not found", 404);
        };
        if item.target_actor_uri != actor_url(&config, requester.username()) {
            return action_not_allowed_response();
        }
        enqueue_delete_feature_authorization_activity(
            &db,
            &config,
            &requester,
            &remote_collection,
            &item,
        )
        .await?;
        if !revoke_remote_collection_item(&db, &collection_id, &item_id).await? {
            return Response::error("collection item not found", 404);
        }
        return Ok(Response::empty()?.with_status(200));
    };
    let Some(item) = collection_item_by_id(&db, &collection_id, &item_id).await? else {
        return Response::error("collection item not found", 404);
    };
    if !can_revoke_collection_item(&requester, &item) {
        return action_not_allowed_response();
    }
    if !revoke_collection_item(&db, &collection_id, &item_id).await? {
        return Response::error("collection item not found", 404);
    }
    Ok(Response::empty()?.with_status(200))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_config() -> cfwdon_core::AppConfig {
        cfwdon_core::AppConfig::new("https://social.example", "cfwdon", "test")
    }

    fn fixture_owner() -> cfwdon_domain::LocalAccount {
        cfwdon_domain::LocalAccount::from_record(cfwdon_domain::LocalAccountRecord::test_fixture(
            "acct-1", "alice",
        ))
    }

    fn fixture_collection_row() -> CollectionRow {
        CollectionRow {
            id: "collection-1".to_owned(),
            account_id: "acct-1".to_owned(),
            name: "Art".to_owned(),
            description: "Sketches".to_owned(),
            language: Some("en".to_owned()),
            sensitive: 0,
            discoverable: 1,
            tag_name: Some("art".to_owned()),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
            updated_at: "2026-01-02T00:00:00.000Z".to_owned(),
        }
    }

    fn fixture_remote_collection_row(id: &str, created_at: &str) -> RemoteCollectionRow {
        RemoteCollectionRow {
            id: id.to_owned(),
            actor_uri: "https://remote.example/users/alice".to_owned(),
            uri: format!("https://remote.example/users/alice/collections/{id}"),
            name: "Remote Art".to_owned(),
            description: String::new(),
            language: None,
            sensitive: 0,
            discoverable: 1,
            tag_name: None,
            url: None,
            published_at: None,
            remote_updated_at: None,
            created_at: created_at.to_owned(),
            updated_at: created_at.to_owned(),
        }
    }

    fn remote_actor_profile_fixture(actor_uri: &str) -> RemoteActorProfile {
        RemoteActorProfile {
            actor_uri: actor_uri.to_owned(),
            username: "alice".to_owned(),
            domain: "remote.example".to_owned(),
            locked: false,
            bot: false,
            discoverable: true,
            indexable: true,
            inbox_uri: "https://remote.example/users/alice/inbox".to_owned(),
            shared_inbox_uri: Some("https://remote.example/inbox".to_owned()),
            public_key_id: "https://remote.example/users/alice#main-key".to_owned(),
            public_key_pem: "pem".to_owned(),
            display_name: "Alice".to_owned(),
            summary_html: String::new(),
            profile_url: Some("https://remote.example/@alice".to_owned()),
            avatar_url: None,
            header_url: None,
        }
    }

    #[test]
    fn collection_list_document_uses_upstream_root_key() {
        let document = collection_list_document(vec![serde_json::json!({ "id": "collection-1" })]);

        assert_eq!(
            document.pointer("/collections/0/id"),
            Some(&serde_json::json!("collection-1"))
        );
    }

    #[test]
    fn collection_response_documents_use_upstream_root_keys() {
        let collection = collection_response_document(serde_json::json!({ "id": "collection-1" }));
        let item = collection_item_response_document(serde_json::json!({ "id": "item-1" }));

        assert_eq!(
            collection.pointer("/collection/id"),
            Some(&serde_json::json!("collection-1"))
        );
        assert_eq!(
            item.pointer("/collection_item/id"),
            Some(&serde_json::json!("item-1"))
        );
    }

    #[test]
    fn collection_language_validation_matches_supported_locales() {
        assert!(collection_language_is_supported(
            &canonical_collection_language("EN")
        ));
        assert!(collection_language_is_supported(
            &canonical_collection_language("zh_yue")
        ));
        assert!(!collection_language_is_supported(
            &canonical_collection_language("randomstuff")
        ));

        let details = validate_collection_request(
            &CollectionRequest {
                name: Some("Art".to_owned()),
                language: Some(canonical_collection_language("randomstuff")),
                ..CollectionRequest::default()
            },
            true,
        );

        assert_eq!(
            details.get("language"),
            Some(&vec!["is invalid".to_owned()])
        );
    }

    #[test]
    fn validation_error_codes_match_mastodon_formatter_style() {
        assert_eq!(validation_error_code("can't be blank"), "ERR_BLANK");
        assert_eq!(
            validation_error_code("is too long (maximum is 40 characters)"),
            "ERR_TOO_LONG"
        );
        assert_eq!(validation_error_code("is invalid"), "ERR_INVALID");
        assert_eq!(
            validation_error_code("are too many (maximum is 25)"),
            "ERR_TOO_MANY"
        );
    }

    #[test]
    fn collection_notification_policy_action_prefers_drop_over_filter() {
        let action = merge_collection_notification_policy_action(
            CollectionNotificationPolicyAction::Deliver,
            "filter",
            true,
        );
        assert_eq!(action, CollectionNotificationPolicyAction::Filter);

        let action = merge_collection_notification_policy_action(action, "drop", true);
        assert_eq!(action, CollectionNotificationPolicyAction::Drop);

        let action = merge_collection_notification_policy_action(action, "filter", true);
        assert_eq!(action, CollectionNotificationPolicyAction::Drop);
    }

    #[test]
    fn collection_offset_link_header_preserves_filters_and_adds_next_prev() {
        let url = url::Url::parse(
            "https://social.example/api/v1_alpha/accounts/acct-1/collections?limit=5&offset=10&tag=art",
        )
        .unwrap();
        let header = build_collection_offset_link_header_for_url(&url, 5, 10, 5, 30).unwrap();

        assert!(header.contains("rel=\"next\""));
        assert!(header.contains("rel=\"prev\""));
        assert!(header.contains("tag=art"));
        assert!(header.contains("limit=5"));
        assert!(header.contains("offset=15"));
        assert!(header.contains("offset=5"));
    }

    #[test]
    fn collection_offset_link_header_omits_next_on_last_page() {
        let url = url::Url::parse(
            "https://social.example/api/v1_alpha/accounts/acct-1/collections?limit=5&offset=10",
        )
        .unwrap();
        let header = build_collection_offset_link_header_for_url(&url, 5, 10, 3, 13).unwrap();

        assert!(!header.contains("rel=\"next\""));
        assert!(header.contains("rel=\"prev\""));
    }

    #[test]
    fn in_collection_page_entries_sort_remote_and_local_together() {
        let mut local = fixture_collection_row();
        local.id = "local-newer".to_owned();
        local.created_at = "2026-01-03T00:00:00.000Z".to_owned();
        let mut entries = vec![
            InCollectionPageEntry::Remote(fixture_remote_collection_row(
                "remote-middle",
                "2026-01-02T00:00:00.000Z",
            )),
            InCollectionPageEntry::Local(local),
            InCollectionPageEntry::Remote(fixture_remote_collection_row(
                "remote-older",
                "2026-01-01T00:00:00.000Z",
            )),
        ];

        sort_in_collection_page_entries(&mut entries);

        let ids = entries
            .iter()
            .map(|entry| in_collection_entry_sort_key(entry).1.to_owned())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["local-newer", "remote-middle", "remote-older"]);
    }

    #[test]
    fn collection_feature_request_activity_matches_upstream_shape() {
        let item = CollectionItemRow {
            id: "item-1".to_owned(),
            target_account_ref: "r_remote".to_owned(),
            state: "pending".to_owned(),
            activity_uri: None,
            feature_authorization: None,
            created_at: "2026-01-02T00:00:00.000Z".to_owned(),
        };
        let activity = build_collection_feature_request_activity(
            &fixture_config(),
            &fixture_owner(),
            "collection-1",
            &item,
            "https://remote.example/users/bob",
        );

        assert_eq!(activity["type"], "FeatureRequest");
        assert_eq!(activity["object"], "https://remote.example/users/bob");
        assert_eq!(
            activity["instrument"],
            "https://social.example/users/alice/collections/collection-1"
        );
        assert_eq!(
            activity["id"],
            "https://social.example/users/alice/collections/collection-1/items/item-1#feature_request"
        );
    }

    #[test]
    fn remote_collection_helpers_accept_mastodon_activitypub_shape() {
        let actor_uri = "https://remote.example/users/alice";
        let object = serde_json::json!({
            "id": "https://remote.example/users/alice/collections/1",
            "type": "FeaturedCollection",
            "name": "Art",
            "attributedTo": actor_uri,
            "summaryMap": { "es": "Boceto" },
            "topic": { "type": "Hashtag", "name": "#Art" },
            "orderedItems": [{
                "id": "https://remote.example/users/alice/collections/1/items/1",
                "type": "FeaturedItem",
                "featuredObject": "https://social.example/users/bob"
            }]
        });

        assert!(collection_object_attributed_to_actor(&object, actor_uri));
        assert!(is_remote_actor_collections_target(
            actor_uri,
            "https://remote.example/users/alice/collections/featured"
        ));
        assert_eq!(
            featured_collection_description(&object),
            ("Boceto".to_owned(), Some("es".to_owned()))
        );
        assert_eq!(
            featured_collection_tag_name(&object),
            Some("art".to_owned())
        );
        assert_eq!(featured_collection_items(&object).len(), 1);
    }

    #[test]
    fn remote_collection_draft_from_object_extracts_storage_fields() {
        let actor = remote_actor_profile_fixture("https://remote.example/users/alice");
        let object = serde_json::json!({
            "id": "https://remote.example/users/alice/collections/1",
            "type": "FeaturedCollection",
            "name": " Art ",
            "attributedTo": actor.actor_uri,
            "summary": "Sketches",
            "contentMap": { "fr": "Dessins" },
            "sensitive": true,
            "discoverable": false,
            "topic": { "type": "Hashtag", "name": "#Art" },
            "url": { "id": "https://remote.example/@alice/collections/art" },
            "published": "2025-01-02T03:04:05Z",
            "updated": "2025-01-03T03:04:05Z",
            "items": []
        });

        let draft = remote_collection_draft_from_object(&actor, &object).unwrap();
        assert_eq!(
            draft.id,
            remote_account_rest_id("https://remote.example/users/alice/collections/1")
        );
        assert_eq!(draft.actor_uri, actor.actor_uri);
        assert_eq!(
            draft.uri,
            "https://remote.example/users/alice/collections/1"
        );
        assert_eq!(draft.name, "Art");
        assert_eq!(draft.description, "Sketches");
        assert_eq!(draft.language.as_deref(), None);
        assert!(draft.sensitive);
        assert!(!draft.discoverable);
        assert_eq!(draft.tag_name.as_deref(), Some("art"));
        assert_eq!(
            draft.url.as_deref(),
            Some("https://remote.example/@alice/collections/art")
        );
        assert_eq!(draft.published_at.as_deref(), Some("2025-01-02T03:04:05Z"));
        assert_eq!(
            draft.remote_updated_at.as_deref(),
            Some("2025-01-03T03:04:05Z")
        );
        assert!(draft.includes_items);
    }

    #[test]
    fn feature_authorization_document_requires_matching_edges() {
        let document = serde_json::json!({
            "id": "https://remote.example/users/bob/feature_authorizations/1",
            "type": "FeatureAuthorization",
            "interactingObject": "https://remote.example/users/alice/collections/1",
            "interactionTarget": { "id": "https://social.example/users/bob" }
        });

        assert!(feature_authorization_matches_document(
            &document,
            "https://remote.example/users/bob/feature_authorizations/1",
            "https://remote.example/users/alice/collections/1",
            "https://social.example/users/bob"
        ));
        assert!(!feature_authorization_matches_document(
            &document,
            "https://remote.example/users/bob/feature_authorizations/1",
            "https://remote.example/users/alice/collections/2",
            "https://social.example/users/bob"
        ));

        let snake_case_document = serde_json::json!({
            "id": "https://remote.example/users/bob/feature_authorizations/1",
            "type": "FeatureAuthorization",
            "interacting_object": { "id": "https://remote.example/users/alice/collections/1" },
            "interaction_target": "https://social.example/users/bob"
        });
        assert!(feature_authorization_matches_document(
            &snake_case_document,
            "https://remote.example/users/bob/feature_authorizations/1",
            "https://remote.example/users/alice/collections/1",
            "https://social.example/users/bob"
        ));
    }

    #[test]
    fn delete_feature_authorization_activity_matches_upstream_shape() {
        let config = fixture_config();
        let owner = fixture_owner();
        let collection =
            fixture_remote_collection_row("remote-collection", "2026-01-01T00:00:00.000Z");
        let item = RemoteCollectionItemRow {
            id: "remote-item".to_owned(),
            uri: Some("https://remote.example/users/alice/collections/1/items/1".to_owned()),
            target_actor_uri: actor_url(&config, owner.username()),
            state: "accepted".to_owned(),
            feature_authorization: Some(
                "https://social.example/users/bob/feature_authorizations/1".to_owned(),
            ),
            approval_last_verified_at: None,
            published_at: None,
            created_at: "2026-01-02T00:00:00.000Z".to_owned(),
        };

        let activity =
            build_delete_feature_authorization_activity(&config, &owner, &collection, &item);

        assert_eq!(activity["type"], "Delete");
        assert_eq!(activity["actor"], "https://social.example/users/alice");
        assert_eq!(
            activity["object"]["type"],
            serde_json::json!("FeatureAuthorization")
        );
        assert_eq!(
            activity["object"]["interactingObject"],
            serde_json::json!(collection.uri)
        );
        assert_eq!(
            local_collection_id_from_uri(
                &config,
                "https://social.example/users/alice/collections/collection-1",
            ),
            Some("collection-1".to_owned())
        );
    }

    #[test]
    fn collection_update_significance_matches_notification_fields() {
        let existing = fixture_collection_row();

        assert!(!collection_update_is_significant(
            &existing,
            &CollectionRequest {
                language: Some("ja".to_owned()),
                discoverable: Some(false),
                ..CollectionRequest::default()
            }
        ));
        assert!(collection_update_is_significant(
            &existing,
            &CollectionRequest {
                name: Some("New Art".to_owned()),
                ..CollectionRequest::default()
            }
        ));
        assert!(collection_update_is_significant(
            &existing,
            &CollectionRequest {
                description: Some("Paintings".to_owned()),
                ..CollectionRequest::default()
            }
        ));
        assert!(collection_update_is_significant(
            &existing,
            &CollectionRequest {
                sensitive: Some(true),
                ..CollectionRequest::default()
            }
        ));
        assert!(collection_update_is_significant(
            &existing,
            &CollectionRequest {
                tag_name: Some("painting".to_owned()),
                ..CollectionRequest::default()
            }
        ));
    }

    #[test]
    fn collection_activity_update_tracks_all_updateable_fields() {
        let existing = fixture_collection_row();

        assert!(collection_update_requires_activity(
            &existing,
            &CollectionRequest {
                language: Some("ja".to_owned()),
                ..CollectionRequest::default()
            }
        ));
        assert!(collection_update_requires_activity(
            &existing,
            &CollectionRequest {
                discoverable: Some(false),
                ..CollectionRequest::default()
            }
        ));
        assert!(!collection_update_requires_activity(
            &existing,
            &CollectionRequest::default()
        ));
    }

    #[test]
    fn collection_item_revoke_policy_allows_featured_local_account_only() {
        let owner = fixture_owner();
        let target = cfwdon_domain::LocalAccount::from_record({
            let mut record = cfwdon_domain::LocalAccountRecord::test_fixture("acct-2", "bob");
            record.access_email = "bob@example.com".to_owned();
            record
        });
        let item = CollectionItemRow {
            id: "item-1".to_owned(),
            target_account_ref: target.id().to_owned(),
            state: "accepted".to_owned(),
            activity_uri: None,
            feature_authorization: None,
            created_at: "2026-01-02T00:00:00.000Z".to_owned(),
        };

        assert!(can_revoke_collection_item(&target, &item));
        assert!(!can_revoke_collection_item(&owner, &item));
    }
}
