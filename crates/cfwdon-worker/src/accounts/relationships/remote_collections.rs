use super::collections::CollectionAccountEntry;
use crate::{
    MastodonAccountResponse, Result, fetch_activitypub_document_with_context,
    fetch_remote_activitypub_document, fetch_remote_actor_profile_with_context,
    find_local_account_response_by_actor_uri, find_remote_actor_by_actor_uri, upsert_remote_actor,
};
use futures_util::{StreamExt, stream};
use std::collections::HashSet;

const REMOTE_FOLLOW_COLLECTION_PAGE_FETCH_LIMIT: usize = 8;
const REMOTE_FOLLOW_ACCOUNT_RESOLVE_CONCURRENCY: usize = 8;

#[derive(Clone, Debug, PartialEq)]
enum RemoteFollowCollectionReference {
    Uri(String),
}

async fn resolve_remote_follow_collection_account(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    reference: &RemoteFollowCollectionReference,
) -> Result<Option<MastodonAccountResponse>> {
    let RemoteFollowCollectionReference::Uri(actor_uri) = reference;
    if let Some(account) = find_local_account_response_by_actor_uri(db, config, actor_uri).await? {
        return Ok(Some(account));
    }

    if let Some(actor) = find_remote_actor_by_actor_uri(db, actor_uri).await? {
        return Ok(Some(MastodonAccountResponse::from_remote_actor(&actor)));
    }

    let fetched = match fetch_remote_actor_profile_with_context(actor_uri, None).await {
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
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    actor_uri: &str,
    collection_field: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Option<Vec<CollectionAccountEntry>>> {
    let actor_document = match fetch_activitypub_document_with_context(actor_uri, None).await {
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
