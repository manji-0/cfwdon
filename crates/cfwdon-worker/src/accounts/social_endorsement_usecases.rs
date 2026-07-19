use crate::{
    CursorAccountCollection, MastodonAccountResponse, RemoteActorRow, find_local_account_response,
    find_local_account_response_by_actor_uri, find_remote_actor_by_actor_uri,
    find_remote_actor_by_profile_url_or_actor_uri, is_activitypub_actor_type,
    list_endorsed_accounts_for_owner, parse_remote_actor_profile_document,
    refreshed_remote_actor_response, upserted_remote_actor_response,
    validate_remote_actor_profile_urls,
};
use std::collections::HashSet;
use worker::{D1Database, Error, Result};

const REMOTE_ENDORSEMENT_PAGE_FETCH_LIMIT: usize = 8;

#[derive(Clone, Debug, PartialEq)]
enum RemoteEndorsementReference {
    Uri(String),
    EmbeddedActor(serde_json::Value),
}

pub(crate) async fn list_local_endorsement_accounts(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    owner_account_id: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<CursorAccountCollection> {
    let entries =
        list_endorsed_accounts_for_owner(db, owner_account_id, limit, max_id, since_id).await?;
    let mut accounts = Vec::new();
    for entry in &entries {
        if let Some(target_account_id) = entry.target_account_id.as_deref()
            && let Some(account) =
                find_local_account_response(db, config, target_account_id).await?
        {
            accounts.push(account);
            continue;
        }

        if let Some(actor) = find_remote_actor_by_actor_uri(db, &entry.target_actor_uri).await?
            && let Some(account) =
                refreshed_remote_endorsement_account_response(db, config, &actor).await?
        {
            accounts.push(account);
        }
    }

    Ok(CursorAccountCollection {
        accounts,
        first_cursor: entries.first().map(|entry| entry.cursor_id),
        last_cursor: entries.last().map(|entry| entry.cursor_id),
        has_next_page: entries.len() as u32 >= limit,
    })
}

pub(crate) async fn list_remote_endorsement_accounts(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    actor_uri: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<CursorAccountCollection> {
    let actor_document = match crate::fetch_remote_activitypub_document(actor_uri).await {
        Ok(document) => document,
        Err(_) => return Ok(empty_social_endorsement_collection()),
    };
    let Some(collection_uri) = extract_remote_endorsement_collection_uri(&actor_document) else {
        return Ok(empty_social_endorsement_collection());
    };
    let references: Vec<RemoteEndorsementReference> =
        fetch_remote_endorsement_collection_item_references(&collection_uri)
            .await
            .unwrap_or_default();

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

    Ok(CursorAccountCollection {
        first_cursor: paged.first().map(|(cursor_id, _)| *cursor_id),
        last_cursor: paged.last().map(|(cursor_id, _)| *cursor_id),
        accounts: paged.into_iter().map(|(_, account)| account).collect(),
        has_next_page: has_next,
    })
}

fn empty_social_endorsement_collection() -> CursorAccountCollection {
    CursorAccountCollection {
        accounts: Vec::new(),
        first_cursor: None,
        last_cursor: None,
        has_next_page: false,
    }
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
    let collection = crate::fetch_remote_activitypub_document(collection_uri).await?;
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
        let page = crate::fetch_remote_activitypub_document(&page_uri).await?;
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
    db: &D1Database,
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
        if let Some(account) =
            find_local_account_response_by_actor_uri(db, config, &profile.actor_uri).await?
        {
            return Ok(Some(account));
        }
        return Ok(Some(upserted_remote_actor_response(db, &profile).await?));
    }

    let RemoteEndorsementReference::Uri(reference) = reference else {
        return Ok(None);
    };
    if let Some(account) = find_local_account_response_by_actor_uri(db, config, reference).await? {
        return Ok(Some(account));
    }

    if let Some(actor) = find_remote_actor_by_profile_url_or_actor_uri(db, reference).await? {
        return refreshed_remote_endorsement_account_response(db, config, &actor).await;
    }

    let profile = match crate::fetch_remote_actor_profile(reference).await {
        Ok(profile) => profile,
        Err(_) => return Ok(None),
    };
    Ok(Some(upserted_remote_actor_response(db, &profile).await?))
}

async fn refreshed_remote_endorsement_account_response(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    actor: &RemoteActorRow,
) -> Result<Option<MastodonAccountResponse>> {
    Ok(Some(
        refreshed_remote_actor_response(db, config, actor, None).await?,
    ))
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
