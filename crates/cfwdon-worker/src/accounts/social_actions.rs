use crate::{
    AccountReference, MastodonAccountResponse, RemoteActorRow, Request, Response, Result,
    RouteContext, actor_url, build_internal_cursor_link_header, build_relationship_for_target,
    fetch_remote_activitypub_document, fetch_remote_actor_profile, find_account_by_id,
    find_account_by_username, find_follow_by_target, find_remote_actor_by_actor_uri,
    find_remote_actor_by_profile_url_or_actor_uri, is_activitypub_actor_type,
    list_endorsed_accounts_for_owner, load_account_stats, load_config,
    local_username_from_actor_uri, parse_internal_pagination_id,
    parse_remote_actor_profile_document, require_authenticated_local_account,
    resolve_account_reference, set_account_email_subscription, set_account_endorsement,
    set_account_note, upsert_remote_actor, validate_remote_actor_profile_urls,
};
use serde::Deserialize;
use std::collections::HashSet;
use worker::Error;

#[derive(Debug, Default, Deserialize)]
struct AccountCollectionQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    max_id: Option<String>,
    #[serde(rename = "since_id")]
    since_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct NoteAccountRequest {
    comment: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct EmailSubscriptionRequest {
    email_notifications: Option<bool>,
}

const REMOTE_ENDORSEMENT_PAGE_FETCH_LIMIT: usize = 8;

#[derive(Clone, Debug, PartialEq)]
enum RemoteEndorsementReference {
    Uri(String),
    EmbeddedActor(serde_json::Value),
}

async fn resolve_relationship_target(
    req: &Request,
    ctx: &RouteContext<()>,
) -> Result<
    Option<(
        worker::D1Database,
        cfwdon_core::AppConfig,
        cfwdon_domain::LocalAccount,
        Option<String>,
        String,
        String,
    )>,
> {
    let config = load_config(ctx);
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Ok(None),
    };
    let target = match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => (
            Some(target.id.clone()),
            target.id,
            actor_url(&config, &target.username),
        ),
        Some(AccountReference::Remote(actor)) => (
            None,
            crate::remote_account_rest_id(&actor.actor_uri),
            actor.actor_uri,
        ),
        None => return Err(Error::RustError("account not found".to_owned())),
    };
    Ok(Some((db, config, viewer, target.0, target.1, target.2)))
}

pub(crate) async fn endorsements_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    build_endorsements_response(&req, &db, &config, &viewer.id, limit, max_id, since_id).await
}

pub(crate) async fn account_endorsements_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(account)) => {
            build_endorsements_response(&req, &db, &config, &account.id, limit, max_id, since_id)
                .await
        }
        Some(AccountReference::Remote(actor)) => {
            build_remote_endorsements_response(
                &req,
                &db,
                &config,
                &actor.actor_uri,
                limit,
                max_id,
                since_id,
            )
            .await
        }
        None => Response::error("account not found", 404),
    }
}

async fn build_remote_endorsements_response(
    req: &Request,
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    actor_uri: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Response> {
    let actor_document = match fetch_remote_activitypub_document(actor_uri).await {
        Ok(document) => document,
        Err(_) => return Response::from_json(&Vec::<serde_json::Value>::new()),
    };
    let Some(collection_uri) = extract_remote_endorsement_collection_uri(&actor_document) else {
        return Response::from_json(&Vec::<serde_json::Value>::new());
    };
    let references =
        match fetch_remote_endorsement_collection_item_references(&collection_uri).await {
            Ok(references) => references,
            Err(_) => Vec::new(),
        };

    let mut resolved_accounts = Vec::new();
    for reference in references {
        if let Some(account) = resolve_remote_endorsement_account(db, config, &reference).await? {
            resolved_accounts.push(account);
        }
    }

    let total = resolved_accounts.len() as i64;
    let mut paged = resolved_accounts
        .into_iter()
        .enumerate()
        .filter_map(|(index, account)| {
            let cursor_id = total - index as i64;
            if max_id.is_some_and(|value| cursor_id >= value)
                || since_id.is_some_and(|value| cursor_id <= value)
            {
                return None;
            }
            Some((cursor_id, account))
        })
        .collect::<Vec<_>>();
    let has_next = paged.len() as u32 > limit;
    if has_next {
        paged.truncate(limit as usize);
    }

    let first_cursor = paged.first().map(|(cursor_id, _)| *cursor_id);
    let last_cursor = paged.last().map(|(cursor_id, _)| *cursor_id);
    let response = paged
        .into_iter()
        .map(|(_, account)| account)
        .collect::<Vec<_>>();

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        req,
        limit,
        first_cursor,
        last_cursor,
        has_next,
        max_id.is_some() || since_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }

    builder.from_json(&response)
}

fn extract_remote_endorsement_collection_uri(actor_document: &serde_json::Value) -> Option<String> {
    for field in ["featuredProfiles", "featured_profiles", "featured"] {
        if let Some(uri) = extract_remote_endorsement_reference(actor_document.get(field)) {
            return Some(uri);
        }
    }
    None
}

async fn fetch_remote_endorsement_collection_item_references(
    collection_uri: &str,
) -> Result<Vec<RemoteEndorsementReference>> {
    let collection = fetch_remote_activitypub_document(collection_uri).await?;
    let mut seen_items = HashSet::new();
    let mut items = Vec::new();
    append_remote_endorsement_item_references(&mut items, &mut seen_items, &collection);
    if !items.is_empty() && collection.get("first").is_none() {
        return Ok(items);
    }

    let mut seen_pages = HashSet::new();
    let mut next_page_uri = if let Some(first_page) = collection.get("first") {
        if let Some(first_page_uri) = extract_remote_endorsement_page_reference(Some(first_page)) {
            Some(first_page_uri)
        } else {
            append_remote_endorsement_item_references(&mut items, &mut seen_items, first_page);
            extract_remote_endorsement_page_reference(first_page.get("next"))
        }
    } else {
        extract_remote_endorsement_page_reference(collection.get("next"))
    };

    while let Some(page_uri) = next_page_uri.take() {
        if seen_pages.len() >= REMOTE_ENDORSEMENT_PAGE_FETCH_LIMIT
            || !seen_pages.insert(page_uri.clone())
        {
            break;
        }
        let page = fetch_remote_activitypub_document(&page_uri).await?;
        append_remote_endorsement_item_references(&mut items, &mut seen_items, &page);
        next_page_uri = extract_remote_endorsement_page_reference(page.get("next"));
    }

    Ok(items)
}

fn append_remote_endorsement_item_references(
    items: &mut Vec<RemoteEndorsementReference>,
    seen_items: &mut HashSet<String>,
    collection: &serde_json::Value,
) {
    for item in extract_remote_endorsement_item_references(collection) {
        let key = item.key();
        if seen_items.insert(key) {
            items.push(item);
        }
    }
}

fn extract_remote_endorsement_item_references(
    collection: &serde_json::Value,
) -> Vec<RemoteEndorsementReference> {
    let Some(items) = collection
        .get("orderedItems")
        .or_else(|| collection.get("items"))
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };

    let mut references = Vec::new();
    for item in items {
        if let Some(reference) = extract_remote_endorsement_item_reference(Some(item)) {
            references.push(reference);
        }
    }
    references
}

fn extract_remote_endorsement_page_reference(value: Option<&serde_json::Value>) -> Option<String> {
    extract_remote_endorsement_reference(value)
}

fn extract_remote_endorsement_reference(value: Option<&serde_json::Value>) -> Option<String> {
    let candidate = match value? {
        serde_json::Value::String(url) => url.clone(),
        serde_json::Value::Object(map) => {
            if let Some(value) = map
                .get("id")
                .or_else(|| map.get("url"))
                .or_else(|| map.get("href"))
                .and_then(serde_json::Value::as_str)
            {
                value.to_owned()
            } else {
                extract_remote_endorsement_reference(map.get("object"))?
            }
        }
        _ => return None,
    };
    let trimmed = candidate.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn extract_remote_endorsement_item_reference(
    value: Option<&serde_json::Value>,
) -> Option<RemoteEndorsementReference> {
    match value? {
        serde_json::Value::Object(map) => {
            if is_activitypub_actor_type(map.get("type").and_then(serde_json::Value::as_str))
                && map.get("inbox").is_some()
                && map.get("publicKey").is_some()
            {
                return Some(RemoteEndorsementReference::EmbeddedActor(
                    serde_json::Value::Object(map.clone()),
                ));
            }
            if let Some(reference) = extract_remote_endorsement_item_reference(map.get("object")) {
                return Some(reference);
            }
            extract_remote_endorsement_reference(value).map(RemoteEndorsementReference::Uri)
        }
        _ => extract_remote_endorsement_reference(value).map(RemoteEndorsementReference::Uri),
    }
}

async fn resolve_remote_endorsement_account(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    reference: &RemoteEndorsementReference,
) -> Result<Option<MastodonAccountResponse>> {
    if let RemoteEndorsementReference::EmbeddedActor(actor_document) = reference {
        let fallback_actor_uri = extract_remote_endorsement_reference(Some(actor_document))
            .ok_or_else(|| {
                Error::RustError("embedded actor endorsement is missing id".to_owned())
            })?;
        let profile = match parse_remote_actor_profile_document(actor_document, &fallback_actor_uri)
        {
            Ok(profile) => profile,
            Err(_) => return Ok(None),
        };
        if validate_remote_actor_profile_urls(&profile).await.is_err() {
            return Ok(None);
        }
        if let Some(username) = local_username_from_actor_uri(config, &profile.actor_uri)
            && let Some(account) = find_account_by_username(db, &username).await?
        {
            let stats = load_account_stats(db, &account.id).await?;
            return Ok(Some(MastodonAccountResponse::from_account_with_stats(
                &account, config, &stats,
            )));
        }
        upsert_remote_actor(db, &profile).await?;
        let response = match find_remote_actor_by_actor_uri(db, &profile.actor_uri).await? {
            Some(actor) => MastodonAccountResponse::from_remote_actor(&actor),
            None => MastodonAccountResponse::from_remote_actor_profile(&profile),
        };
        return Ok(Some(response));
    }

    let RemoteEndorsementReference::Uri(reference) = reference else {
        return Ok(None);
    };
    if let Some(username) = local_username_from_actor_uri(config, reference)
        && let Some(account) = find_account_by_username(db, &username).await?
    {
        let stats = load_account_stats(db, &account.id).await?;
        return Ok(Some(MastodonAccountResponse::from_account_with_stats(
            &account, config, &stats,
        )));
    }

    if let Some(actor) = find_remote_actor_by_profile_url_or_actor_uri(db, reference).await? {
        return refreshed_remote_endorsement_account_response(db, actor).await;
    }

    let profile = match fetch_remote_actor_profile(reference).await {
        Ok(profile) => profile,
        Err(_) => return Ok(None),
    };
    upsert_remote_actor(db, &profile).await?;
    let response = match find_remote_actor_by_actor_uri(db, &profile.actor_uri).await? {
        Some(actor) => MastodonAccountResponse::from_remote_actor(&actor),
        None => MastodonAccountResponse::from_remote_actor_profile(&profile),
    };
    Ok(Some(response))
}

async fn refreshed_remote_endorsement_account_response(
    db: &worker::D1Database,
    actor: RemoteActorRow,
) -> Result<Option<MastodonAccountResponse>> {
    let profile = match fetch_remote_actor_profile(&actor.actor_uri).await {
        Ok(profile) => profile,
        Err(_) => return Ok(Some(MastodonAccountResponse::from_remote_actor(&actor))),
    };
    upsert_remote_actor(db, &profile).await?;
    let response = match find_remote_actor_by_actor_uri(db, &profile.actor_uri).await? {
        Some(actor) => MastodonAccountResponse::from_remote_actor(&actor),
        None => MastodonAccountResponse::from_remote_actor_profile(&profile),
    };
    Ok(Some(response))
}

impl RemoteEndorsementReference {
    fn key(&self) -> String {
        match self {
            Self::Uri(uri) => format!("uri:{uri}"),
            Self::EmbeddedActor(actor) => extract_remote_endorsement_reference(Some(actor))
                .map(|uri| format!("actor:{uri}"))
                .unwrap_or_else(|| format!("actor:{actor}")),
        }
    }
}

async fn build_endorsements_response(
    req: &Request,
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    owner_account_id: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Response> {
    let entries =
        list_endorsed_accounts_for_owner(db, owner_account_id, limit, max_id, since_id).await?;
    let mut response = Vec::new();
    for entry in &entries {
        if let Some(target_account_id) = entry.target_account_id.as_deref()
            && let Some(account) = find_account_by_id(db, target_account_id).await?
        {
            let stats = load_account_stats(db, &account.id).await?;
            response.push(MastodonAccountResponse::from_account_with_stats(
                &account, config, &stats,
            ));
            continue;
        }

        if let Some(actor) = find_remote_actor_by_actor_uri(db, &entry.target_actor_uri).await?
            && let Some(account) = refreshed_remote_endorsement_account_response(db, actor).await?
        {
            response.push(account);
        }
    }

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        req,
        limit,
        entries.first().map(|entry| entry.cursor_id),
        entries.last().map(|entry| entry.cursor_id),
        entries.len() as u32 >= limit,
        max_id.is_some() || since_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }

    builder.from_json(&response)
}

pub(crate) async fn pin_account_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    endorse_or_pin_account_response(req, ctx, true).await
}

pub(crate) async fn unpin_account_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    endorse_or_pin_account_response(req, ctx, false).await
}

pub(crate) async fn endorse_account_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    endorse_or_pin_account_response(req, ctx, true).await
}

pub(crate) async fn unendorse_account_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    endorse_or_pin_account_response(req, ctx, false).await
}

async fn endorse_or_pin_account_response(
    req: Request,
    ctx: RouteContext<()>,
    endorsed: bool,
) -> Result<Response> {
    let Some((db, config, viewer, target_account_id, target_id, target_actor_uri)) =
        (match resolve_relationship_target(&req, &ctx).await {
            Ok(values) => values,
            Err(Error::RustError(message)) if message == "account not found" => {
                return Response::error("account not found", 404);
            }
            Err(error) => return Err(error),
        })
    else {
        return Response::error("Cloudflare Access authentication required", 401);
    };

    let Some(follow) = find_follow_by_target(&db, &viewer.id, &target_actor_uri).await? else {
        return Response::error(
            "Validation failed: You must be already following the person you want to endorse",
            422,
        );
    };
    if follow.state != "accepted" {
        return Response::error(
            "Validation failed: You must be already following the person you want to endorse",
            422,
        );
    }

    set_account_endorsement(
        &db,
        &viewer.id,
        target_account_id.as_deref(),
        &target_actor_uri,
        endorsed,
    )
    .await?;

    let relationship =
        build_relationship_for_target(&db, &config, &viewer, &target_id, &target_actor_uri).await?;
    Response::from_json(&relationship)
}

async fn parse_note_request(req: &mut Request) -> std::result::Result<String, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let comment = if content_type.contains("application/json") {
        req.json::<NoteAccountRequest>()
            .await
            .map_err(|error| format!("invalid JSON note payload: {error}"))?
            .comment
    } else if content_type.trim().is_empty() {
        None
    } else {
        req.form_data()
            .await
            .map_err(|error| format!("invalid form note payload: {error}"))?
            .get_field("comment")
    };

    Ok(comment.unwrap_or_default().trim().to_owned())
}

pub(crate) async fn note_account_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some((db, config, viewer, target_account_id, target_id, target_actor_uri)) =
        (match resolve_relationship_target(&req, &ctx).await {
            Ok(values) => values,
            Err(Error::RustError(message)) if message == "account not found" => {
                return Response::error("account not found", 404);
            }
            Err(error) => return Err(error),
        })
    else {
        return Response::error("Cloudflare Access authentication required", 401);
    };

    let note = match parse_note_request(&mut req).await {
        Ok(note) => note,
        Err(message) => return Response::error(&message, 400),
    };

    set_account_note(
        &db,
        &viewer.id,
        target_account_id.as_deref(),
        &target_actor_uri,
        &note,
    )
    .await?;

    let relationship =
        build_relationship_for_target(&db, &config, &viewer, &target_id, &target_actor_uri).await?;
    Response::from_json(&relationship)
}

async fn parse_email_subscription_request(req: &mut Request) -> std::result::Result<bool, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.contains("application/json") {
        let payload = req
            .json::<EmailSubscriptionRequest>()
            .await
            .map_err(|error| format!("invalid JSON email subscription payload: {error}"))?;
        return Ok(payload.email_notifications.unwrap_or(true));
    }

    if content_type.trim().is_empty() {
        return Ok(true);
    }

    let value = req
        .form_data()
        .await
        .map_err(|error| format!("invalid form email subscription payload: {error}"))?
        .get_field("email_notifications");
    match value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => Ok(true),
        Some("1" | "true" | "TRUE" | "on" | "ON" | "yes" | "YES") => Ok(true),
        Some("0" | "false" | "FALSE" | "off" | "OFF" | "no" | "NO") => Ok(false),
        Some(_) => Err("invalid email_notifications value".to_owned()),
    }
}

pub(crate) async fn account_email_subscriptions_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some((db, _config, viewer, target_account_id, target_id, target_actor_uri)) =
        (match resolve_relationship_target(&req, &ctx).await {
            Ok(values) => values,
            Err(Error::RustError(message)) if message == "account not found" => {
                return Response::error("account not found", 404);
            }
            Err(error) => return Err(error),
        })
    else {
        return Response::error("Cloudflare Access authentication required", 401);
    };

    let enabled = match parse_email_subscription_request(&mut req).await {
        Ok(enabled) => enabled,
        Err(message) => return Response::error(&message, 400),
    };

    set_account_email_subscription(
        &db,
        &viewer.id,
        target_account_id.as_deref(),
        &target_actor_uri,
        enabled,
    )
    .await?;

    Response::from_json(&serde_json::json!({
        "id": target_id,
        "email_notifications": enabled,
    }))
}

#[cfg(test)]
mod tests {
    use super::{
        RemoteEndorsementReference, extract_remote_endorsement_collection_uri,
        extract_remote_endorsement_item_references, extract_remote_endorsement_page_reference,
    };

    #[test]
    fn remote_endorsement_collection_uri_prefers_profiles_extension() {
        let document = serde_json::json!({
            "featured": "https://remote.example/users/alice/collections/featured",
            "featuredProfiles": {
                "id": "https://remote.example/users/alice/collections/featured_profiles"
            }
        });

        assert_eq!(
            extract_remote_endorsement_collection_uri(&document).as_deref(),
            Some("https://remote.example/users/alice/collections/featured_profiles")
        );
    }

    #[test]
    fn remote_endorsement_item_references_extracts_references_in_order() {
        let collection = serde_json::json!({
            "orderedItems": [
                "https://social.example/@alice",
                { "id": "https://remote.example/users/bob" },
                { "object": { "url": "https://remote.example/@carol" } },
                { "id": "https://remote.example/users/bob" },
                { "type": "Note", "content": "ignore me" }
            ]
        });

        assert_eq!(
            extract_remote_endorsement_item_references(&collection),
            vec![
                RemoteEndorsementReference::Uri("https://social.example/@alice".to_owned()),
                RemoteEndorsementReference::Uri("https://remote.example/users/bob".to_owned()),
                RemoteEndorsementReference::Uri("https://remote.example/@carol".to_owned()),
                RemoteEndorsementReference::Uri("https://remote.example/users/bob".to_owned()),
            ]
        );
    }

    #[test]
    fn remote_endorsement_item_references_preserve_embedded_actor_objects() {
        let collection = serde_json::json!({
            "orderedItems": [
                {
                    "type": "Person",
                    "id": "https://remote.example/users/dana",
                    "preferredUsername": "dana",
                    "inbox": "https://remote.example/users/dana/inbox",
                    "publicKey": {
                        "id": "https://remote.example/users/dana#main-key",
                        "publicKeyPem": "pem"
                    }
                },
                {
                    "type": "Add",
                    "object": {
                        "type": "Service",
                        "id": "https://remote.example/actors/app",
                        "preferredUsername": "app",
                        "inbox": "https://remote.example/actors/app/inbox",
                        "publicKey": {
                            "id": "https://remote.example/actors/app#main-key",
                            "publicKeyPem": "pem"
                        }
                    }
                }
            ]
        });

        let references = extract_remote_endorsement_item_references(&collection);
        assert!(matches!(
            references.first(),
            Some(RemoteEndorsementReference::EmbeddedActor(actor))
                if actor.get("id").and_then(serde_json::Value::as_str)
                    == Some("https://remote.example/users/dana")
        ));
        assert!(matches!(
            references.get(1),
            Some(RemoteEndorsementReference::EmbeddedActor(actor))
                if actor.get("id").and_then(serde_json::Value::as_str)
                    == Some("https://remote.example/actors/app")
        ));
    }

    #[test]
    fn remote_endorsement_page_reference_extracts_next_page_uri() {
        let page = serde_json::json!({
            "id": "https://remote.example/users/alice/featured?page=1",
            "next": {
                "id": "https://remote.example/users/alice/featured?page=2"
            }
        });

        assert_eq!(
            extract_remote_endorsement_page_reference(page.get("next")).as_deref(),
            Some("https://remote.example/users/alice/featured?page=2")
        );
    }
}
