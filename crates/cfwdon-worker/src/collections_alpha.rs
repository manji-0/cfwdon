use crate::{
    AccountReference, LocalApiAuthentication, RemoteActorProfile, RemoteActorRow, Request,
    Response, Result, actor_url, app_bearer_token_from_request, authenticate_local_api_request,
    fetch_remote_activitypub_document, find_follow_by_target, is_blocking_actor,
    local_username_from_actor_uri, oauth_access_token_has_any_scope, parse_optional_bool,
    remote_account_rest_id, upsert_remote_actor,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use worker::d1::D1Type;

mod activity;
mod documents;
mod inbox;
mod notifications;
mod queries;
mod routes;

pub(in crate::collections_alpha) use activity::{
    enqueue_collection_add_activity, enqueue_collection_feature_request_activity,
    enqueue_collection_item_add_activity, enqueue_collection_item_remove_activity,
    enqueue_collection_remove_activity, enqueue_collection_update_activity,
    enqueue_delete_feature_authorization_activity,
};

pub(crate) use documents::local_collection_id_from_uri;
pub(in crate::collections_alpha) use documents::{
    collection_document, collection_item_document, collection_item_response_document,
    collection_item_uri, collection_list_document, collection_response_document, collection_uri,
    collection_with_accounts_document, remote_collection_document, remote_collection_item_document,
    remote_collection_with_accounts_document,
};

pub(in crate::collections_alpha) use queries::{
    collection_item_by_feature_request_uri, collection_item_by_id, collection_row_by_id,
    count_collection_rows_for_account, count_in_collection_rows,
    count_remote_collection_rows_for_actor, count_remote_in_collection_rows, delete_collection,
    delete_collection_item, delete_remote_collection_by_uri,
    delete_remote_collection_item_by_object, insert_collection, insert_collection_item,
    list_collection_items, list_collection_rows_for_account, list_local_in_collection_rows,
    list_remote_collection_items, list_remote_collection_items_due_for_approval_revalidation,
    list_remote_collection_rows_for_actor, list_remote_in_collection_rows,
    list_stale_remote_collection_items_for_approval_revalidation, remote_collection_item_by_id,
    remote_collection_row_by_id, remote_collection_row_by_uri, revoke_collection_item,
    revoke_remote_collection_item, sort_in_collection_page_entries, update_collection,
    update_collection_item_feature_request_uri, update_collection_item_feature_state,
    update_remote_collection_item_approval_verification, upsert_remote_collection_draft,
};

pub(crate) use inbox::{
    handle_inbox_collection_add, handle_inbox_collection_feature_accept,
    handle_inbox_collection_feature_authorization_delete, handle_inbox_collection_feature_reject,
    handle_inbox_collection_remove, handle_inbox_collection_update,
};
pub(crate) use notifications::collect_collection_notification_entries;
pub(crate) use routes::{
    alpha_account_collections_response, alpha_account_in_collections_response,
    alpha_collection_response, create_alpha_collection_item_response,
    create_alpha_collection_response, delete_alpha_collection_item_response,
    delete_alpha_collection_response, revoke_alpha_collection_item_response,
    update_alpha_collection_response,
};

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

fn can_revoke_collection_item(
    requester: &cfwdon_domain::LocalAccount,
    item: &CollectionItemRow,
) -> bool {
    item.target_account_ref == requester.id()
}

#[cfg(test)]
mod tests {
    use super::activity::{
        build_collection_feature_request_activity, build_delete_feature_authorization_activity,
    };
    use super::notifications::merge_collection_notification_policy_action;
    use super::routes::build_collection_offset_link_header_for_url;
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
            .map(|entry| match entry {
                InCollectionPageEntry::Local(row) => row.id.as_str(),
                InCollectionPageEntry::Remote(row) => row.id.as_str(),
            })
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
