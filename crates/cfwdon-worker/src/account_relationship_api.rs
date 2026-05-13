use super::{
    MastodonAccountResponse, RemoteActorProfile, Request, Response, Result, RouteContext,
    build_internal_cursor_link_header, fetch_remote_activitypub_document,
    fetch_remote_actor_profile, find_account_by_id, find_account_by_username,
    find_authenticated_local_account, find_remote_actor_by_actor_uri,
    find_remote_actor_by_username_domain, is_activitypub_actor_type, list_blocks_for_account,
    list_familiar_local_accounts_for_local_target, list_familiar_local_accounts_for_remote_target,
    list_familiar_remote_actors_for_local_target, list_local_followers_for_account,
    list_local_followers_for_remote_actor, list_local_following_for_account,
    list_local_following_for_remote_actor, list_mutes_for_account,
    list_remote_followers_for_account, list_remote_following_for_account, load_account_stats,
    load_config, local_username_from_actor_uri, parse_internal_pagination_id, parse_lookup_handle,
    parse_remote_actor_profile_document, remote_account_rest_id, resolve_account_reference,
    upsert_remote_actor, upsert_remote_actors, validate_remote_actor_profile_urls,
};
use crate::{AccountReference, actor_url, build_relationship_for_target};
use futures_util::{StreamExt, stream};
use std::collections::HashSet;

const FAMILIAR_FOLLOWERS_LIMIT: usize = 3;
const REMOTE_FOLLOW_COLLECTION_PAGE_FETCH_LIMIT: usize = 8;
const REMOTE_FOLLOW_ACCOUNT_RESOLVE_CONCURRENCY: usize = 8;

#[derive(Debug, Default, serde::Deserialize)]
struct AccountCollectionQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    max_id: Option<String>,
    #[serde(rename = "since_id")]
    since_id: Option<String>,
    #[serde(rename = "min_id")]
    min_id: Option<String>,
}

pub(crate) async fn account_relationships(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let relationships =
        futures_util::future::try_join_all(parse_relationship_query_ids(&req)?.into_iter().map(
            |account_id| {
                let db = &db;
                let config = &config;
                let viewer = &viewer;
                async move {
                    match resolve_requested_account_reference(&db, &config, &account_id).await? {
                        Some(AccountReference::Local(target)) => Ok::<_, worker::Error>(Some(
                            build_relationship_for_target(
                                &db,
                                &config,
                                &viewer,
                                &target.id,
                                &actor_url(&config, &target.username),
                            )
                            .await?,
                        )),
                        Some(AccountReference::Remote(actor)) => Ok::<_, worker::Error>(Some(
                            build_relationship_for_target(
                                &db,
                                &config,
                                &viewer,
                                &remote_account_rest_id(&actor.actor_uri),
                                &actor.actor_uri,
                            )
                            .await?,
                        )),
                        None => Ok::<_, worker::Error>(None),
                    }
                }
            },
        ))
        .await?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    Response::from_json(&relationships)
}

fn build_familiar_followers_entry(
    account_id: &str,
    accounts: Vec<MastodonAccountResponse>,
) -> serde_json::Value {
    serde_json::json!({
        "id": account_id,
        "accounts": accounts,
    })
}

pub(crate) async fn familiar_followers_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let mut response = Vec::new();
    for requested_account_id in parse_relationship_query_ids(&req)? {
        let mut accounts = Vec::new();
        let mut seen_ids = HashSet::new();

        match resolve_account_reference(&db, &requested_account_id).await? {
            Some(AccountReference::Local(target)) => {
                for account in list_familiar_local_accounts_for_local_target(
                    &db,
                    &viewer.id,
                    &target.id,
                    FAMILIAR_FOLLOWERS_LIMIT as u32,
                )
                .await?
                {
                    let stats = load_account_stats(&db, &account.id).await?;
                    let response_account =
                        MastodonAccountResponse::from_account_with_stats(&account, &config, &stats);
                    if seen_ids.insert(response_account.id.clone()) {
                        accounts.push(response_account);
                    }
                    if accounts.len() >= FAMILIAR_FOLLOWERS_LIMIT {
                        break;
                    }
                }
                if accounts.len() < FAMILIAR_FOLLOWERS_LIMIT {
                    for actor in list_familiar_remote_actors_for_local_target(
                        &db,
                        &viewer.id,
                        &target.id,
                        (FAMILIAR_FOLLOWERS_LIMIT - accounts.len()) as u32,
                    )
                    .await?
                    {
                        let response_account = MastodonAccountResponse::from_remote_actor(&actor);
                        if seen_ids.insert(response_account.id.clone()) {
                            accounts.push(response_account);
                        }
                        if accounts.len() >= FAMILIAR_FOLLOWERS_LIMIT {
                            break;
                        }
                    }
                }
            }
            Some(AccountReference::Remote(actor)) => {
                for account in list_familiar_local_accounts_for_remote_target(
                    &db,
                    &viewer.id,
                    &actor.actor_uri,
                    FAMILIAR_FOLLOWERS_LIMIT as u32,
                )
                .await?
                {
                    let stats = load_account_stats(&db, &account.id).await?;
                    let response_account =
                        MastodonAccountResponse::from_account_with_stats(&account, &config, &stats);
                    if seen_ids.insert(response_account.id.clone()) {
                        accounts.push(response_account);
                    }
                    if accounts.len() >= FAMILIAR_FOLLOWERS_LIMIT {
                        break;
                    }
                }
            }
            None => {}
        }

        response.push(build_familiar_followers_entry(
            &requested_account_id,
            accounts,
        ));
    }

    Response::from_json(&response)
}

#[derive(Debug)]
struct CollectionAccountEntry {
    cursor_id: i64,
    created_at: String,
    account: MastodonAccountResponse,
}

#[derive(Debug)]
struct ResolvedRemoteFollowAccount {
    account: MastodonAccountResponse,
    profile_to_upsert: Option<RemoteActorProfile>,
}

#[derive(Clone, Debug, PartialEq)]
enum RemoteFollowCollectionReference {
    Uri(String),
    EmbeddedActor(serde_json::Value),
}

async fn resolve_requested_account_reference(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    account_id: &str,
) -> Result<Option<AccountReference>> {
    if let Some(reference) = resolve_account_reference(db, account_id).await? {
        return Ok(Some(reference));
    }

    let handle = match parse_lookup_handle(account_id, config) {
        Ok(handle) => handle,
        Err(_) => return Ok(None),
    };

    if handle.is_local_to(&config.instance_domain) {
        return Ok(find_account_by_username(db, &handle.username)
            .await?
            .map(AccountReference::Local));
    }

    let Some(domain) = handle.domain.as_deref() else {
        return Ok(None);
    };
    Ok(
        find_remote_actor_by_username_domain(db, &handle.username, domain)
            .await?
            .map(AccountReference::Remote),
    )
}

fn finalize_collection_response(
    req: &Request,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
    mut entries: Vec<CollectionAccountEntry>,
) -> Result<Response> {
    entries.retain(|entry| max_id.is_none_or(|value| entry.cursor_id < value));
    entries.retain(|entry| since_id.is_none_or(|value| entry.cursor_id > value));
    entries.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.cursor_id.cmp(&left.cursor_id))
    });
    let has_next_page = entries.len() > limit as usize;
    if entries.len() > limit as usize {
        entries.truncate(limit as usize);
    }

    let first_id = entries.first().map(|entry| entry.cursor_id);
    let last_id = entries.last().map(|entry| entry.cursor_id);
    let response = entries
        .into_iter()
        .map(|entry| entry.account)
        .collect::<Vec<_>>();

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        req,
        limit,
        first_id,
        last_id,
        has_next_page,
        max_id.is_some() || since_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }

    builder.from_json(&response)
}

async fn remote_follow_account_response(
    db: &worker::D1Database,
    actor_uri: &str,
) -> Result<Option<MastodonAccountResponse>> {
    match fetch_remote_actor_profile(actor_uri).await {
        Ok(profile) => {
            upsert_remote_actor(db, &profile).await?;
            match find_remote_actor_by_actor_uri(db, &profile.actor_uri).await? {
                Some(actor) => Ok(Some(MastodonAccountResponse::from_remote_actor(&actor))),
                None => Ok(Some(MastodonAccountResponse::from_remote_actor_profile(
                    &profile,
                ))),
            }
        }
        Err(_) => Ok(find_remote_actor_by_actor_uri(db, actor_uri)
            .await?
            .map(|actor| MastodonAccountResponse::from_remote_actor(&actor))),
    }
}

async fn resolve_remote_follow_collection_account(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    reference: &RemoteFollowCollectionReference,
) -> Result<Option<ResolvedRemoteFollowAccount>> {
    if let RemoteFollowCollectionReference::EmbeddedActor(actor_document) = reference {
        let fallback_actor_uri = extract_remote_follow_collection_reference(Some(actor_document))
            .ok_or_else(|| {
            worker::Error::RustError("embedded remote follow actor is missing id".to_owned())
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
            return Ok(Some(ResolvedRemoteFollowAccount {
                account: MastodonAccountResponse::from_account_with_stats(&account, config, &stats),
                profile_to_upsert: None,
            }));
        }
        return Ok(Some(ResolvedRemoteFollowAccount {
            account: MastodonAccountResponse::from_remote_actor_profile(&profile),
            profile_to_upsert: Some(profile),
        }));
    }

    let RemoteFollowCollectionReference::Uri(actor_uri) = reference else {
        return Ok(None);
    };
    if let Some(username) = local_username_from_actor_uri(config, actor_uri)
        && let Some(account) = find_account_by_username(db, &username).await?
    {
        let stats = load_account_stats(db, &account.id).await?;
        return Ok(Some(ResolvedRemoteFollowAccount {
            account: MastodonAccountResponse::from_account_with_stats(&account, config, &stats),
            profile_to_upsert: None,
        }));
    }

    let profile = match fetch_remote_actor_profile(actor_uri).await {
        Ok(profile) => profile,
        Err(_) => {
            return Ok(find_remote_actor_by_actor_uri(db, actor_uri)
                .await?
                .map(|actor| ResolvedRemoteFollowAccount {
                    account: MastodonAccountResponse::from_remote_actor(&actor),
                    profile_to_upsert: None,
                }));
        }
    };
    if let Some(username) = local_username_from_actor_uri(config, &profile.actor_uri)
        && let Some(account) = find_account_by_username(db, &username).await?
    {
        let stats = load_account_stats(db, &account.id).await?;
        return Ok(Some(ResolvedRemoteFollowAccount {
            account: MastodonAccountResponse::from_account_with_stats(&account, config, &stats),
            profile_to_upsert: None,
        }));
    }
    Ok(Some(ResolvedRemoteFollowAccount {
        account: MastodonAccountResponse::from_remote_actor_profile(&profile),
        profile_to_upsert: Some(profile),
    }))
}

async fn remote_follow_collection_entries(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    actor_uri: &str,
    collection_field: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Option<Vec<CollectionAccountEntry>>> {
    let actor_document = match fetch_remote_activitypub_document(actor_uri).await {
        Ok(document) => document,
        Err(_) => return Ok(None),
    };
    let Some(collection_uri) =
        extract_remote_follow_collection_reference(actor_document.get(collection_field))
    else {
        return Ok(Some(Vec::new()));
    };
    let references = match fetch_remote_follow_collection_item_references(&collection_uri).await {
        Ok(references) => references,
        Err(_) => return Ok(None),
    };

    let resolved_entries = stream::iter(page_remote_follow_collection_references(
        references, limit, max_id, since_id,
    ))
    .map(|(cursor_id, reference)| async move {
        let resolved = resolve_remote_follow_collection_account(db, config, &reference).await?;
        Ok::<_, worker::Error>(resolved.map(|resolved| (cursor_id, resolved)))
    })
    .buffered(REMOTE_FOLLOW_ACCOUNT_RESOLVE_CONCURRENCY)
    .collect::<Vec<_>>()
    .await;

    let mut entries = Vec::new();
    let mut profiles_to_upsert = Vec::new();
    let mut seen_profile_uris = HashSet::new();
    for entry in resolved_entries {
        if let Some((cursor_id, resolved)) = entry? {
            if let Some(profile) = resolved.profile_to_upsert
                && seen_profile_uris.insert(profile.actor_uri.clone())
            {
                profiles_to_upsert.push(profile);
            }
            entries.push(CollectionAccountEntry {
                cursor_id,
                created_at: String::new(),
                account: resolved.account,
            });
        }
    }
    upsert_remote_actors(db, &profiles_to_upsert).await?;
    Ok(Some(entries))
}

fn page_remote_follow_collection_references(
    references: Vec<RemoteFollowCollectionReference>,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Vec<(i64, RemoteFollowCollectionReference)> {
    let total = references.len() as i64;
    references
        .into_iter()
        .enumerate()
        .filter_map(|(index, reference)| {
            let cursor_id = total - index as i64;
            if max_id.is_some_and(|value| cursor_id >= value)
                || since_id.is_some_and(|value| cursor_id <= value)
            {
                return None;
            }
            Some((cursor_id, reference))
        })
        .take(limit as usize)
        .collect()
}

async fn fetch_remote_follow_collection_item_references(
    collection_uri: &str,
) -> Result<Vec<RemoteFollowCollectionReference>> {
    let collection = fetch_remote_activitypub_document(collection_uri).await?;
    let mut seen_items = HashSet::new();
    let mut items = Vec::new();
    append_remote_follow_collection_item_references(&mut items, &mut seen_items, &collection);
    if !items.is_empty() && collection.get("first").is_none() {
        return Ok(items);
    }

    let mut seen_pages = HashSet::new();
    let mut next_page_uri = if let Some(first_page) = collection.get("first") {
        if let Some(first_page_uri) =
            extract_remote_follow_collection_page_reference(Some(first_page))
        {
            Some(first_page_uri)
        } else {
            append_remote_follow_collection_item_references(
                &mut items,
                &mut seen_items,
                first_page,
            );
            extract_remote_follow_collection_page_reference(first_page.get("next"))
        }
    } else {
        extract_remote_follow_collection_page_reference(collection.get("next"))
    };

    while let Some(page_uri) = next_page_uri.take() {
        if seen_pages.len() >= REMOTE_FOLLOW_COLLECTION_PAGE_FETCH_LIMIT
            || !seen_pages.insert(page_uri.clone())
        {
            break;
        }
        let page = fetch_remote_activitypub_document(&page_uri).await?;
        append_remote_follow_collection_item_references(&mut items, &mut seen_items, &page);
        next_page_uri = extract_remote_follow_collection_page_reference(page.get("next"));
    }

    Ok(items)
}

fn append_remote_follow_collection_item_references(
    items: &mut Vec<RemoteFollowCollectionReference>,
    seen_items: &mut HashSet<String>,
    collection: &serde_json::Value,
) {
    for item in extract_remote_follow_collection_item_references(collection) {
        let key = item.key();
        if seen_items.insert(key) {
            items.push(item);
        }
    }
}

fn extract_remote_follow_collection_item_references(
    collection: &serde_json::Value,
) -> Vec<RemoteFollowCollectionReference> {
    let Some(items) = collection
        .get("orderedItems")
        .or_else(|| collection.get("items"))
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };

    let mut references = Vec::new();
    for item in items {
        if let Some(reference) = extract_remote_follow_collection_item_reference(Some(item)) {
            references.push(reference);
        }
    }
    references
}

fn extract_remote_follow_collection_item_reference(
    value: Option<&serde_json::Value>,
) -> Option<RemoteFollowCollectionReference> {
    match value? {
        serde_json::Value::Object(map) => {
            if is_activitypub_actor_type(map.get("type").and_then(serde_json::Value::as_str))
                && map.get("inbox").is_some()
                && map.get("publicKey").is_some()
            {
                return Some(RemoteFollowCollectionReference::EmbeddedActor(
                    serde_json::Value::Object(map.clone()),
                ));
            }
            if let Some(reference) =
                extract_remote_follow_collection_item_reference(map.get("object"))
            {
                return Some(reference);
            }
            extract_remote_follow_collection_reference(value)
                .map(RemoteFollowCollectionReference::Uri)
        }
        _ => extract_remote_follow_collection_reference(value)
            .map(RemoteFollowCollectionReference::Uri),
    }
}

fn extract_remote_follow_collection_page_reference(
    value: Option<&serde_json::Value>,
) -> Option<String> {
    extract_remote_follow_collection_reference(value)
}

fn extract_remote_follow_collection_reference(value: Option<&serde_json::Value>) -> Option<String> {
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
                extract_remote_follow_collection_reference(map.get("object"))?
            }
        }
        _ => return None,
    };
    let trimmed = candidate.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

impl RemoteFollowCollectionReference {
    fn key(&self) -> String {
        match self {
            Self::Uri(uri) => format!("uri:{uri}"),
            Self::EmbeddedActor(actor) => extract_remote_follow_collection_reference(Some(actor))
                .map(|uri| format!("actor:{uri}"))
                .unwrap_or_else(|| format!("actor:{actor}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_follow_collection_references_extract_items_in_order() {
        let collection = serde_json::json!({
            "orderedItems": [
                "https://remote.example/users/alice",
                { "id": "https://remote.example/users/bob" },
                { "object": { "url": "https://remote.example/@carol" } },
                { "id": "https://remote.example/users/bob" },
                { "type": "Note", "content": "ignore me" }
            ]
        });

        assert_eq!(
            extract_remote_follow_collection_item_references(&collection),
            vec![
                RemoteFollowCollectionReference::Uri(
                    "https://remote.example/users/alice".to_owned()
                ),
                RemoteFollowCollectionReference::Uri("https://remote.example/users/bob".to_owned()),
                RemoteFollowCollectionReference::Uri("https://remote.example/@carol".to_owned()),
                RemoteFollowCollectionReference::Uri("https://remote.example/users/bob".to_owned()),
            ]
        );
    }

    #[test]
    fn remote_follow_collection_references_preserve_embedded_actor_objects() {
        let collection = serde_json::json!({
            "items": [
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
                    "type": "Announce",
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

        let references = extract_remote_follow_collection_item_references(&collection);
        assert!(matches!(
            references.first(),
            Some(RemoteFollowCollectionReference::EmbeddedActor(actor))
                if actor.get("id").and_then(serde_json::Value::as_str)
                    == Some("https://remote.example/users/dana")
        ));
        assert!(matches!(
            references.get(1),
            Some(RemoteFollowCollectionReference::EmbeddedActor(actor))
                if actor.get("id").and_then(serde_json::Value::as_str)
                    == Some("https://remote.example/actors/app")
        ));
    }

    #[test]
    fn remote_follow_collection_page_reference_extracts_next_page_uri() {
        let page = serde_json::json!({
            "id": "https://remote.example/users/alice/followers?page=1",
            "next": {
                "id": "https://remote.example/users/alice/followers?page=2"
            }
        });

        assert_eq!(
            extract_remote_follow_collection_page_reference(page.get("next")).as_deref(),
            Some("https://remote.example/users/alice/followers?page=2")
        );
    }

    #[test]
    fn remote_follow_collection_references_page_before_resolution() {
        let references = vec![
            RemoteFollowCollectionReference::Uri("https://remote.example/users/a".to_owned()),
            RemoteFollowCollectionReference::Uri("https://remote.example/users/b".to_owned()),
            RemoteFollowCollectionReference::Uri("https://remote.example/users/c".to_owned()),
            RemoteFollowCollectionReference::Uri("https://remote.example/users/d".to_owned()),
        ];

        assert_eq!(
            page_remote_follow_collection_references(references.clone(), 2, None, None),
            vec![
                (
                    4,
                    RemoteFollowCollectionReference::Uri(
                        "https://remote.example/users/a".to_owned()
                    )
                ),
                (
                    3,
                    RemoteFollowCollectionReference::Uri(
                        "https://remote.example/users/b".to_owned()
                    )
                ),
            ]
        );
        assert_eq!(
            page_remote_follow_collection_references(references, 2, Some(3), None),
            vec![
                (
                    2,
                    RemoteFollowCollectionReference::Uri(
                        "https://remote.example/users/c".to_owned()
                    )
                ),
                (
                    1,
                    RemoteFollowCollectionReference::Uri(
                        "https://remote.example/users/d".to_owned()
                    )
                ),
            ]
        );
    }
}

pub(crate) fn parse_relationship_query_ids(req: &Request) -> Result<Vec<String>> {
    let url = req.url()?;
    let mut ids = Vec::new();

    for (key, value) in url.query_pairs() {
        if key == "id[]" || key == "id" {
            let value = value.trim().to_owned();
            if !value.is_empty() {
                ids.push(value);
            }
        }
    }

    Ok(ids)
}

pub(crate) async fn account_followers_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    let min_id = parse_internal_pagination_id(query.min_id.as_deref(), "min_id")?;
    let since_id = since_id.or(min_id);
    let account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing account id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;

    let mut entries = Vec::new();
    match resolve_requested_account_reference(&db, &config, &account_id).await? {
        Some(AccountReference::Local(account)) => {
            for follower in list_local_followers_for_account(&db, &account.id).await? {
                if let Some(author) = find_account_by_id(&db, &follower.account_id).await? {
                    let stats = load_account_stats(&db, &author.id).await?;
                    entries.push(CollectionAccountEntry {
                        cursor_id: follower.cursor_id,
                        created_at: follower.created_at,
                        account: MastodonAccountResponse::from_account_with_stats(
                            &author, &config, &stats,
                        ),
                    });
                }
            }
            for follower in list_remote_followers_for_account(&db, &account.id).await? {
                if let Some(account) =
                    remote_follow_account_response(&db, &follower.actor_uri).await?
                {
                    entries.push(CollectionAccountEntry {
                        cursor_id: follower.cursor_id,
                        created_at: follower.created_at,
                        account,
                    });
                }
            }
        }
        Some(AccountReference::Remote(actor)) => {
            match remote_follow_collection_entries(
                &db,
                &config,
                &actor.actor_uri,
                "followers",
                limit.saturating_add(1),
                max_id,
                since_id,
            )
            .await?
            {
                Some(remote_entries) => entries = remote_entries,
                None => {
                    for follower in
                        list_local_followers_for_remote_actor(&db, &actor.actor_uri).await?
                    {
                        if let Some(account) = find_account_by_id(&db, &follower.account_id).await?
                        {
                            let stats = load_account_stats(&db, &account.id).await?;
                            entries.push(CollectionAccountEntry {
                                cursor_id: follower.cursor_id,
                                created_at: follower.created_at,
                                account: MastodonAccountResponse::from_account_with_stats(
                                    &account, &config, &stats,
                                ),
                            });
                        }
                    }
                }
            }
        }
        None => return Response::error("account not found", 404),
    }

    finalize_collection_response(&req, limit, max_id, since_id, entries)
}

pub(crate) async fn account_following_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    let min_id = parse_internal_pagination_id(query.min_id.as_deref(), "min_id")?;
    let since_id = since_id.or(min_id);
    let account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing account id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;

    let mut entries = Vec::new();
    match resolve_requested_account_reference(&db, &config, &account_id).await? {
        Some(AccountReference::Local(account)) => {
            for followed in list_local_following_for_account(&db, &account.id).await? {
                if let Some(target) = find_account_by_id(&db, &followed.account_id).await? {
                    let stats = load_account_stats(&db, &target.id).await?;
                    entries.push(CollectionAccountEntry {
                        cursor_id: followed.cursor_id,
                        created_at: followed.created_at,
                        account: MastodonAccountResponse::from_account_with_stats(
                            &target, &config, &stats,
                        ),
                    });
                }
            }
            for followed in list_remote_following_for_account(&db, &account.id).await? {
                if let Some(account) =
                    remote_follow_account_response(&db, &followed.actor_uri).await?
                {
                    entries.push(CollectionAccountEntry {
                        cursor_id: followed.cursor_id,
                        created_at: followed.created_at,
                        account,
                    });
                }
            }
        }
        Some(AccountReference::Remote(actor)) => {
            match remote_follow_collection_entries(
                &db,
                &config,
                &actor.actor_uri,
                "following",
                limit.saturating_add(1),
                max_id,
                since_id,
            )
            .await?
            {
                Some(remote_entries) => entries = remote_entries,
                None => {
                    for followed in
                        list_local_following_for_remote_actor(&db, &actor.actor_uri).await?
                    {
                        if let Some(account) = find_account_by_id(&db, &followed.account_id).await?
                        {
                            let stats = load_account_stats(&db, &account.id).await?;
                            entries.push(CollectionAccountEntry {
                                cursor_id: followed.cursor_id,
                                created_at: followed.created_at,
                                account: MastodonAccountResponse::from_account_with_stats(
                                    &account, &config, &stats,
                                ),
                            });
                        }
                    }
                }
            }
        }
        None => return Response::error("account not found", 404),
    }

    finalize_collection_response(&req, limit, max_id, since_id, entries)
}

pub(crate) async fn identity_proofs_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing account id route parameter".to_owned()))?;

    match find_authenticated_local_account(&req, &db, &config).await? {
        Some(_) => {}
        None => return Response::error("Cloudflare Access authentication required", 401),
    }

    if resolve_account_reference(&db, &account_id).await?.is_none() {
        return Response::error("account not found", 404);
    }

    Response::from_json(&Vec::<serde_json::Value>::new())
}

pub(crate) async fn blocks_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    let min_id = parse_internal_pagination_id(query.min_id.as_deref(), "min_id")?;
    let since_id = since_id.or(min_id);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let blocks = list_blocks_for_account(&db, &viewer.id, limit, max_id, since_id).await?;
    let mut response = Vec::new();
    for block in &blocks {
        if let Some(target_account_id) = block.target_account_id.as_deref()
            && let Some(account) = find_account_by_id(&db, target_account_id).await?
        {
            response.push(MastodonAccountResponse::from_account(&account, &config));
            continue;
        }

        if let Some(actor) = find_remote_actor_by_actor_uri(&db, &block.target_actor_uri).await? {
            response.push(MastodonAccountResponse::from_remote_actor(&actor));
        }
    }

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        &req,
        limit,
        blocks.first().map(|block| block.cursor_id),
        blocks.last().map(|block| block.cursor_id),
        blocks.len() as u32 >= limit,
        max_id.is_some() || since_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }

    builder.from_json(&response)
}

pub(crate) async fn mutes_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    let min_id = parse_internal_pagination_id(query.min_id.as_deref(), "min_id")?;
    let since_id = since_id.or(min_id);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let mutes = list_mutes_for_account(&db, &viewer.id, limit, max_id, since_id).await?;
    let mut response = Vec::new();
    for mute in &mutes {
        if let Some(target_account_id) = mute.target_account_id.as_deref()
            && let Some(account) = find_account_by_id(&db, target_account_id).await?
        {
            response.push(MastodonAccountResponse::from_account(&account, &config));
            continue;
        }

        if let Some(actor) = find_remote_actor_by_actor_uri(&db, &mute.target_actor_uri).await? {
            response.push(MastodonAccountResponse::from_remote_actor(&actor));
        }
    }

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        &req,
        limit,
        mutes.first().map(|mute| mute.cursor_id),
        mutes.last().map(|mute| mute.cursor_id),
        mutes.len() as u32 >= limit,
        max_id.is_some() || since_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }

    builder.from_json(&response)
}
