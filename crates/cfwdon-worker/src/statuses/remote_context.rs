use super::{
    AppConfig, D1Database, LocalAccount, MastodonContextResponse, MastodonStatusResponse,
    RemoteActorRow, RemoteStatusRow, build_loaded_local_status_response,
    build_remote_status_response, context_descendant_max_depth, extract_remote_note_object,
    fetch_remote_activitypub_document, fetch_remote_actor_profile, find_account_by_id,
    find_remote_actor_by_actor_uri, find_remote_status_by_object_uri, find_status_by_ap_id,
    find_status_by_id, is_public_activitypub_visibility, list_direct_remote_replies_by_uri,
    resolve_remote_status_by_url, trim_context_ancestors, trim_context_descendants,
    upsert_remote_actor, upsert_remote_status, visibility_from_activitypub_object,
};
use std::collections::HashSet;
use worker::Result;

const REMOTE_CONTEXT_REPLY_PAGE_FETCH_LIMIT: usize = 8;
const REMOTE_CONTEXT_REPLY_ITEM_FETCH_LIMIT: usize = 128;

struct RemoteContextDescendantQueueNode {
    object_uri: String,
    depth: usize,
}

type RemoteContextDescendant = (String, MastodonStatusResponse);

fn next_remote_context_child_depth(max_depth: Option<usize>, depth: usize) -> Option<usize> {
    let child_depth = depth.saturating_add(1);
    if max_depth.is_some_and(|limit| child_depth > limit) {
        None
    } else {
        Some(child_depth)
    }
}

pub(crate) async fn build_remote_status_context(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    root: &RemoteStatusRow,
    root_actor: &RemoteActorRow,
) -> Result<MastodonContextResponse> {
    let is_authenticated = viewer.is_some();
    let ancestors = collect_ancestors_for_remote_root(db, config, viewer, root).await?;

    if viewer.is_some() {
        let _ = hydrate_remote_descendants_for_context(db, config, root, root_actor, 0).await;
    }
    let descendants =
        collect_descendants_for_remote_root(db, config, viewer, root, root_actor).await?;
    Ok(MastodonContextResponse {
        ancestors: trim_context_ancestors(ancestors, is_authenticated),
        descendants,
    })
}

async fn collect_ancestors_for_remote_root(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    root: &RemoteStatusRow,
) -> Result<Vec<MastodonStatusResponse>> {
    let mut ancestors = Vec::new();
    let mut current = root.in_reply_to_uri.clone();
    let mut seen_local_ids = HashSet::new();
    let mut seen_remote_ids = HashSet::new();

    while let Some(object_uri) = current {
        if let Some(local_status) = find_status_by_ap_id(db, &object_uri).await? {
            let mut current_local = Some(local_status);
            while let Some(status) = current_local {
                if !seen_local_ids.insert(status.id.clone()) {
                    break;
                }
                let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
                    break;
                };
                if !is_public_activitypub_visibility(&status.visibility) {
                    break;
                }
                ancestors.push(
                    build_loaded_local_status_response(db, config, viewer, &status, &owner).await?,
                );
                current_local = match status.in_reply_to_id.as_deref() {
                    Some(parent_id) => find_status_by_id(db, parent_id).await?,
                    None => None,
                };
            }
            break;
        }

        let status = match find_remote_status_by_object_uri(db, &object_uri).await? {
            Some(status) => status,
            None if viewer.is_some() => {
                let Some((status, _actor)) =
                    resolve_remote_status_by_url(db, config, &object_uri).await?
                else {
                    break;
                };
                status
            }
            None => break,
        };
        if !seen_remote_ids.insert(status.id.clone()) {
            break;
        }
        if !is_public_activitypub_visibility(&status.visibility) {
            break;
        }
        let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await? else {
            break;
        };
        ancestors.push(build_remote_status_response(db, config, viewer, &status, &actor).await?);
        current = status.in_reply_to_uri.clone();
    }
    ancestors.reverse();
    Ok(ancestors)
}

async fn collect_descendants_for_remote_root(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    root: &RemoteStatusRow,
    _root_actor: &RemoteActorRow,
) -> Result<Vec<MastodonStatusResponse>> {
    let max_depth = context_descendant_max_depth(viewer.is_some());
    let mut descendants = Vec::new();
    let mut queued_uris = vec![RemoteContextDescendantQueueNode {
        object_uri: root.object_uri.clone(),
        depth: 0,
    }];
    let mut seen_remote_ids = HashSet::from([root.id.clone()]);

    while let Some(node) = queued_uris.pop() {
        append_remote_context_child_descendants(
            db,
            config,
            viewer,
            &node,
            max_depth,
            &mut seen_remote_ids,
            &mut queued_uris,
            &mut descendants,
        )
        .await?;
    }

    descendants.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(trim_context_descendants(
        descendants.into_iter().map(|(_, status)| status).collect(),
        viewer.is_some(),
    ))
}

async fn append_remote_context_child_descendants(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    node: &RemoteContextDescendantQueueNode,
    max_depth: Option<usize>,
    seen_remote_ids: &mut HashSet<String>,
    queued_uris: &mut Vec<RemoteContextDescendantQueueNode>,
    descendants: &mut Vec<RemoteContextDescendant>,
) -> Result<()> {
    for (status, actor) in list_direct_remote_replies_by_uri(db, &node.object_uri).await? {
        if !seen_remote_ids.insert(status.id.clone()) {
            continue;
        }
        let Some(child_depth) = next_remote_context_child_depth(max_depth, node.depth) else {
            continue;
        };
        if !is_public_activitypub_visibility(&status.visibility) {
            continue;
        }
        descendants.push((
            status.published_at.clone(),
            build_remote_status_response(db, config, viewer, &status, &actor).await?,
        ));
        queued_uris.push(RemoteContextDescendantQueueNode {
            object_uri: status.object_uri.clone(),
            depth: child_depth,
        });
    }

    Ok(())
}

#[derive(Clone, Debug)]
enum RemoteReplyReference {
    Uri(String),
    Document(serde_json::Value),
}

impl RemoteReplyReference {
    fn key(&self) -> String {
        match self {
            Self::Uri(uri) => format!("uri:{uri}"),
            Self::Document(document) => extract_remote_reply_reference(Some(document))
                .map(|uri| format!("doc:{uri}"))
                .unwrap_or_else(|| format!("doc:{document}")),
        }
    }
}

async fn hydrate_remote_descendants_for_context(
    db: &D1Database,
    config: &AppConfig,
    root: &RemoteStatusRow,
    root_actor: &RemoteActorRow,
    depth: usize,
) -> Result<()> {
    let document = match fetch_remote_activitypub_document(&root.object_uri).await {
        Ok(document) => document,
        Err(_) => return Ok(()),
    };
    let Some(object) = extract_remote_note_object(&document) else {
        return Ok(());
    };
    hydrate_remote_reply_descendants(
        db,
        config,
        object,
        Some(root_actor.actor_uri.as_str()),
        depth,
    )
    .await
}

async fn hydrate_remote_reply_descendants(
    db: &D1Database,
    config: &AppConfig,
    object: &serde_json::Value,
    fallback_actor_uri: Option<&str>,
    depth: usize,
) -> Result<()> {
    if depth >= REMOTE_CONTEXT_REPLY_PAGE_FETCH_LIMIT {
        return Ok(());
    }

    let reply_references = fetch_remote_reply_references(object.get("replies")).await?;
    for reference in reply_references {
        let reply_document = match reference {
            RemoteReplyReference::Document(document) => document,
            RemoteReplyReference::Uri(uri) => {
                let document = match fetch_remote_activitypub_document(&uri).await {
                    Ok(document) => document,
                    Err(_) => continue,
                };
                let Some(object) = extract_remote_note_object(&document) else {
                    continue;
                };
                object.clone()
            }
        };

        if !is_public_activitypub_visibility(&visibility_from_activitypub_object(&reply_document)) {
            continue;
        }
        let actor_uri = reply_document
            .get("attributedTo")
            .and_then(serde_json::Value::as_str)
            .or(fallback_actor_uri);
        let Some(actor_uri) = actor_uri.filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        let actor = match fetch_remote_actor_profile(actor_uri).await {
            Ok(actor) => actor,
            Err(_) => continue,
        };
        upsert_remote_actor(db, &actor).await?;
        upsert_remote_status(db, config, &actor, &reply_document).await?;
        let _ = Box::pin(hydrate_remote_reply_descendants(
            db,
            config,
            &reply_document,
            Some(actor.actor_uri.as_str()),
            depth.saturating_add(1),
        ))
        .await;
    }

    Ok(())
}

async fn fetch_remote_reply_references(
    replies: Option<&serde_json::Value>,
) -> Result<Vec<RemoteReplyReference>> {
    let Some(replies) = replies else {
        return Ok(Vec::new());
    };

    let mut references = Vec::new();
    let mut seen_items = HashSet::new();
    append_remote_reply_references(&mut references, &mut seen_items, replies);
    if !references.is_empty() && replies.get("first").is_none() {
        return Ok(references);
    }

    let mut seen_pages = HashSet::new();
    let mut next_page_uri = if let Some(first_page) = replies.get("first") {
        if let Some(first_page_uri) = extract_remote_reply_page_reference(Some(first_page)) {
            Some(first_page_uri)
        } else {
            append_remote_reply_references(&mut references, &mut seen_items, first_page);
            extract_remote_reply_page_reference(first_page.get("next"))
        }
    } else {
        extract_remote_reply_page_reference(replies.get("next"))
    };

    while let Some(page_uri) = next_page_uri.take() {
        if seen_pages.len() >= REMOTE_CONTEXT_REPLY_PAGE_FETCH_LIMIT
            || !seen_pages.insert(page_uri.clone())
            || references.len() >= REMOTE_CONTEXT_REPLY_ITEM_FETCH_LIMIT
        {
            break;
        }
        let page = match fetch_remote_activitypub_document(&page_uri).await {
            Ok(page) => page,
            Err(_) => break,
        };
        append_remote_reply_references(&mut references, &mut seen_items, &page);
        next_page_uri = extract_remote_reply_page_reference(page.get("next"));
    }

    Ok(references)
}

fn append_remote_reply_references(
    references: &mut Vec<RemoteReplyReference>,
    seen_items: &mut HashSet<String>,
    collection: &serde_json::Value,
) {
    for reference in extract_remote_reply_references(collection) {
        let key = reference.key();
        if seen_items.insert(key) {
            references.push(reference);
            if references.len() >= REMOTE_CONTEXT_REPLY_ITEM_FETCH_LIMIT {
                break;
            }
        }
    }
}

fn extract_remote_reply_references(collection: &serde_json::Value) -> Vec<RemoteReplyReference> {
    let Some(items) = collection
        .get("orderedItems")
        .or_else(|| collection.get("items"))
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };

    let mut references = Vec::new();
    for item in items {
        if let Some(reference) = extract_remote_reply_reference_item(Some(item)) {
            references.push(reference);
        }
    }
    references
}

fn extract_remote_reply_page_reference(value: Option<&serde_json::Value>) -> Option<String> {
    extract_remote_reply_reference(value)
}

fn extract_remote_reply_reference(value: Option<&serde_json::Value>) -> Option<String> {
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
                extract_remote_reply_reference(map.get("object"))?
            }
        }
        _ => return None,
    };
    let trimmed = candidate.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn extract_remote_reply_reference_item(
    value: Option<&serde_json::Value>,
) -> Option<RemoteReplyReference> {
    let value = value?;
    if let Some(object) = extract_remote_note_object(value) {
        return Some(RemoteReplyReference::Document(object.clone()));
    }
    extract_remote_reply_reference(Some(value)).map(RemoteReplyReference::Uri)
}

#[cfg(test)]
mod tests {
    use super::{
        RemoteReplyReference, extract_remote_reply_reference, extract_remote_reply_reference_item,
        extract_remote_reply_references, next_remote_context_child_depth,
    };

    #[test]
    fn remote_reply_reference_extracts_inline_note_and_uri_items() {
        let collection = serde_json::json!({
            "orderedItems": [
                {
                    "type": "Note",
                    "id": "https://remote.example/users/alice/statuses/1",
                    "attributedTo": "https://remote.example/users/alice",
                    "content": "<p>hello</p>"
                },
                "https://remote.example/users/bob/statuses/2"
            ]
        });

        let references = extract_remote_reply_references(&collection);
        assert_eq!(references.len(), 2);
        match &references[0] {
            RemoteReplyReference::Document(document) => assert_eq!(
                document["id"],
                serde_json::json!("https://remote.example/users/alice/statuses/1")
            ),
            RemoteReplyReference::Uri(uri) => panic!("expected inline document, got {uri}"),
        }
        match &references[1] {
            RemoteReplyReference::Uri(uri) => {
                assert_eq!(uri, "https://remote.example/users/bob/statuses/2")
            }
            RemoteReplyReference::Document(document) => {
                panic!("expected uri, got inline document {document}")
            }
        }
    }

    #[test]
    fn remote_reply_reference_recurses_through_wrapped_object() {
        let item = serde_json::json!({
            "type": "Create",
            "object": {
                "type": "Question",
                "id": "https://remote.example/users/alice/statuses/3",
                "attributedTo": "https://remote.example/users/alice",
                "content": "<p>poll</p>"
            }
        });

        let reference = extract_remote_reply_reference_item(Some(&item)).expect("reference");
        match reference {
            RemoteReplyReference::Document(document) => assert_eq!(
                document["id"],
                serde_json::json!("https://remote.example/users/alice/statuses/3")
            ),
            RemoteReplyReference::Uri(uri) => panic!("expected inline document, got {uri}"),
        }

        let page = serde_json::json!({
            "object": {
                "id": "https://remote.example/contexts/replies?page=2"
            }
        });
        assert_eq!(
            extract_remote_reply_reference(Some(&page)).as_deref(),
            Some("https://remote.example/contexts/replies?page=2")
        );
    }

    #[test]
    fn next_remote_context_child_depth_respects_limit() {
        assert_eq!(
            next_remote_context_child_depth(None, usize::MAX),
            Some(usize::MAX)
        );
        assert_eq!(next_remote_context_child_depth(Some(2), 1), Some(2));
        assert_eq!(next_remote_context_child_depth(Some(2), 2), None);
    }
}
