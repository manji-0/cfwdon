use super::collections::CollectionAccountEntry;
use crate::{
    LocalAccount, MastodonAccountResponse, RemoteCollectionFetchContext, Result,
    fetch_activitypub_document_with_context, fetch_remote_actor_profile_with_context,
    find_local_account_response_by_actor_uri, find_remote_actor_by_actor_uri, upsert_remote_actor,
};
use futures_util::{StreamExt, stream};
use std::collections::HashSet;

/// Absolute-position cursors stay stable across lazy page fetches.
const REMOTE_FOLLOW_CURSOR_BASE: i64 = i64::MAX / 4;
const REMOTE_FOLLOW_COLLECTION_PAGE_FETCH_LIMIT: usize = 40;
const REMOTE_FOLLOW_ACCOUNT_RESOLVE_CONCURRENCY: usize = 8;

#[derive(Clone, Debug, PartialEq)]
enum RemoteFollowCollectionReference {
    Uri(String),
}

fn remote_follow_collection_cursor_id(index: usize) -> i64 {
    REMOTE_FOLLOW_CURSOR_BASE - index as i64
}

fn remote_follow_collection_reference_in_page(
    cursor_id: i64,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> bool {
    max_id.is_none_or(|value| cursor_id < value) && since_id.is_none_or(|value| cursor_id > value)
}

fn select_remote_follow_collection_page(
    references: impl IntoIterator<Item = RemoteFollowCollectionReference>,
    start_index: usize,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> (usize, Vec<(i64, RemoteFollowCollectionReference)>, bool) {
    let mut absolute_index = start_index;
    let mut selected = Vec::new();
    let mut exhausted_lower_bound = false;
    for reference in references {
        let cursor_id = remote_follow_collection_cursor_id(absolute_index);
        absolute_index += 1;
        if since_id.is_some_and(|value| cursor_id <= value) {
            exhausted_lower_bound = true;
            break;
        }
        if !remote_follow_collection_reference_in_page(cursor_id, max_id, since_id) {
            continue;
        }
        selected.push((cursor_id, reference));
        if selected.len() >= limit as usize {
            break;
        }
    }
    (absolute_index, selected, exhausted_lower_bound)
}

async fn resolve_remote_follow_collection_account(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    reference: &RemoteFollowCollectionReference,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
) -> Result<Option<MastodonAccountResponse>> {
    let RemoteFollowCollectionReference::Uri(actor_uri) = reference;
    if let Some(account) = find_local_account_response_by_actor_uri(db, config, actor_uri).await? {
        return Ok(Some(account));
    }

    if let Some(actor) = find_remote_actor_by_actor_uri(db, actor_uri).await? {
        return Ok(Some(MastodonAccountResponse::from_remote_actor(&actor)));
    }

    let fetched = match fetch_remote_actor_profile_with_context(actor_uri, fetch_context).await {
        Ok(fetched) => fetched,
        Err(_) => return Ok(None),
    };
    let profile = fetched.profile;
    if let Some(account) =
        find_local_account_response_by_actor_uri(db, config, &profile.actor_uri).await?
    {
        return Ok(Some(account));
    }
    upsert_remote_actor(db, &profile).await?;
    let account = match find_remote_actor_by_actor_uri(db, &profile.actor_uri).await? {
        Some(actor) => MastodonAccountResponse::from_remote_actor(&actor),
        None => MastodonAccountResponse::from_remote_actor_profile(&profile),
    };
    Ok(Some(account))
}

pub(crate) async fn remote_follow_collection_entries(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&LocalAccount>,
    actor_uri: &str,
    collection_field: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Option<Vec<CollectionAccountEntry>>> {
    let fetch_context = RemoteCollectionFetchContext::public(config, db, viewer);
    let fetch_context = Some(&fetch_context);
    let actor_document =
        match fetch_activitypub_document_with_context(actor_uri, fetch_context).await {
            Ok(document) => document,
            Err(_) => return Ok(None),
        };
    let Some(collection_uri) =
        extract_remote_follow_collection_reference(actor_document.get(collection_field))
    else {
        return Ok(Some(Vec::new()));
    };
    let references = match fetch_remote_follow_collection_page_references(
        &collection_uri,
        fetch_context,
        limit,
        max_id,
        since_id,
    )
    .await
    {
        Ok(references) => references,
        Err(_) => return Ok(None),
    };

    let resolved_entries = stream::iter(references)
        .map(|(cursor_id, reference)| async move {
            let resolved =
                resolve_remote_follow_collection_account(db, config, &reference, fetch_context)
                    .await?;
            Ok::<_, worker::Error>(resolved.map(|account| (cursor_id, account)))
        })
        .buffered(REMOTE_FOLLOW_ACCOUNT_RESOLVE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

    let mut entries = Vec::new();
    for entry in resolved_entries {
        if let Some((cursor_id, account)) = entry? {
            entries.push(CollectionAccountEntry {
                cursor_id,
                created_at: String::new(),
                account,
            });
        }
    }
    Ok(Some(entries))
}

async fn fetch_remote_follow_collection_page_references(
    collection_uri: &str,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Vec<(i64, RemoteFollowCollectionReference)>> {
    let collection = fetch_activitypub_document_with_context(collection_uri, fetch_context).await?;
    let mut seen_items = HashSet::new();
    let mut absolute_index = 0usize;
    let mut selected = Vec::new();

    let initial_items =
        take_unique_remote_follow_collection_item_references(&mut seen_items, &collection);
    let (next_index, page_selected, exhausted) = select_remote_follow_collection_page(
        initial_items,
        absolute_index,
        limit,
        max_id,
        since_id,
    );
    absolute_index = next_index;
    selected.extend(page_selected);
    if exhausted || selected.len() >= limit as usize {
        return Ok(selected);
    }
    if !seen_items.is_empty() && collection.get("first").is_none() {
        return Ok(selected);
    }

    let mut seen_pages = HashSet::new();
    let mut next_page_uri = if let Some(first_page) = collection.get("first") {
        if let Some(first_page_uri) =
            extract_remote_follow_collection_page_reference(Some(first_page))
        {
            Some(first_page_uri)
        } else {
            let embedded_items =
                take_unique_remote_follow_collection_item_references(&mut seen_items, first_page);
            let (next_index, page_selected, exhausted) = select_remote_follow_collection_page(
                embedded_items,
                absolute_index,
                limit.saturating_sub(selected.len() as u32),
                max_id,
                since_id,
            );
            absolute_index = next_index;
            selected.extend(page_selected);
            if exhausted || selected.len() >= limit as usize {
                return Ok(selected);
            }
            extract_remote_follow_collection_page_reference(first_page.get("next"))
        }
    } else {
        extract_remote_follow_collection_page_reference(collection.get("next"))
    };

    while let Some(page_uri) = next_page_uri.take() {
        if selected.len() >= limit as usize {
            break;
        }
        if seen_pages.len() >= REMOTE_FOLLOW_COLLECTION_PAGE_FETCH_LIMIT
            || !seen_pages.insert(page_uri.clone())
        {
            break;
        }
        let page = fetch_activitypub_document_with_context(&page_uri, fetch_context).await?;
        let page_items =
            take_unique_remote_follow_collection_item_references(&mut seen_items, &page);
        let (next_index, page_selected, exhausted) = select_remote_follow_collection_page(
            page_items,
            absolute_index,
            limit.saturating_sub(selected.len() as u32),
            max_id,
            since_id,
        );
        absolute_index = next_index;
        selected.extend(page_selected);
        if exhausted || selected.len() >= limit as usize {
            break;
        }
        next_page_uri = extract_remote_follow_collection_page_reference(page.get("next"));
    }

    Ok(selected)
}

fn take_unique_remote_follow_collection_item_references(
    seen_items: &mut HashSet<String>,
    collection: &serde_json::Value,
) -> Vec<RemoteFollowCollectionReference> {
    let mut items = Vec::new();
    for item in extract_remote_follow_collection_item_references(collection) {
        let key = item.key();
        if seen_items.insert(key) {
            items.push(item);
        }
    }
    items
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
    // Never persist embedded actor documents from collections; only resolve by URI.
    match value? {
        serde_json::Value::Object(map) => {
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(path: &str) -> RemoteFollowCollectionReference {
        RemoteFollowCollectionReference::Uri(format!("https://remote.example/users/{path}"))
    }

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
    fn remote_follow_collection_references_resolve_embedded_actors_to_uris() {
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

        assert_eq!(
            extract_remote_follow_collection_item_references(&collection),
            vec![
                RemoteFollowCollectionReference::Uri(
                    "https://remote.example/users/dana".to_owned()
                ),
                RemoteFollowCollectionReference::Uri(
                    "https://remote.example/actors/app".to_owned()
                ),
            ]
        );
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
    fn remote_follow_collection_page_uses_absolute_cursors() {
        let references = vec![uri("a"), uri("b"), uri("c"), uri("d")];
        let (_, first_page, _) =
            select_remote_follow_collection_page(references.clone(), 0, 2, None, None);
        assert_eq!(
            first_page,
            vec![
                (remote_follow_collection_cursor_id(0), uri("a")),
                (remote_follow_collection_cursor_id(1), uri("b")),
            ]
        );

        let max_id = remote_follow_collection_cursor_id(1);
        let (_, second_page, _) =
            select_remote_follow_collection_page(references, 0, 2, Some(max_id), None);
        assert_eq!(
            second_page,
            vec![
                (remote_follow_collection_cursor_id(2), uri("c")),
                (remote_follow_collection_cursor_id(3), uri("d")),
            ]
        );
    }

    #[test]
    fn remote_follow_collection_page_can_resume_from_later_absolute_index() {
        let page_two = vec![uri("c"), uri("d")];
        let (next_index, selected, exhausted) =
            select_remote_follow_collection_page(page_two, 2, 2, None, None);
        assert_eq!(next_index, 4);
        assert!(!exhausted);
        assert_eq!(
            selected,
            vec![
                (remote_follow_collection_cursor_id(2), uri("c")),
                (remote_follow_collection_cursor_id(3), uri("d")),
            ]
        );
    }

    #[test]
    fn remote_follow_collection_page_stops_at_since_id_lower_bound() {
        let references = vec![uri("a"), uri("b"), uri("c"), uri("d")];
        let since_id = remote_follow_collection_cursor_id(1);
        let (next_index, selected, exhausted) =
            select_remote_follow_collection_page(references, 0, 10, None, Some(since_id));
        assert!(exhausted);
        assert_eq!(next_index, 2);
        assert_eq!(
            selected,
            vec![(remote_follow_collection_cursor_id(0), uri("a"))]
        );
    }
}
