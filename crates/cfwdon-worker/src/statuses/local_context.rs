use super::{
    AppConfig, D1Database, LocalAccount, MastodonContextResponse, MastodonStatusResponse,
    StatusRow, actor_url, build_loaded_local_status_response, build_remote_status_response,
    can_view_local_status, context_descendant_max_depth, find_account_by_id, find_status_by_id,
    is_public_activitypub_visibility, list_direct_local_replies, list_direct_remote_replies_by_uri,
    trim_context_ancestors, trim_context_descendants,
};
use std::collections::HashSet;
use worker::Result;

fn local_context_object_uri(
    config: &AppConfig,
    owner: &LocalAccount,
    status: &StatusRow,
) -> String {
    status.ap_id.clone().unwrap_or_else(|| {
        format!(
            "{}/statuses/{}",
            actor_url(config, owner.username()),
            status.id
        )
    })
}

fn context_depth_exceeds_limit(max_depth: Option<usize>, depth: usize) -> bool {
    max_depth.is_some_and(|limit| depth > limit)
}

fn next_context_child_depth(max_depth: Option<usize>, depth: usize) -> Option<usize> {
    let child_depth = depth.saturating_add(1);
    if context_depth_exceeds_limit(max_depth, child_depth) {
        None
    } else {
        Some(child_depth)
    }
}

struct LocalContextQueueNode {
    status_id: String,
    object_uri: String,
    depth: usize,
}

struct RemoteContextQueueNode {
    object_uri: String,
    depth: usize,
}

type ContextDescendant = (String, MastodonStatusResponse);

pub(crate) async fn build_local_status_context(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    root: &StatusRow,
    root_owner: &LocalAccount,
) -> Result<MastodonContextResponse> {
    let is_authenticated = viewer.is_some();
    let mut ancestors = Vec::new();
    let mut current = root.in_reply_to_id.clone();
    let mut seen_local_ids = HashSet::new();

    while let Some(status_id) = current {
        if !seen_local_ids.insert(status_id.clone()) {
            break;
        }
        let Some(status) = find_status_by_id(db, &status_id).await? else {
            break;
        };
        let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
            break;
        };
        if !can_view_local_status(db, &status, viewer, &owner).await? {
            break;
        }
        ancestors
            .push(build_loaded_local_status_response(db, config, viewer, &status, &owner).await?);
        current = status.in_reply_to_id.clone();
    }
    ancestors.reverse();
    let ancestors = trim_context_ancestors(ancestors, is_authenticated);

    let root_uri = local_context_object_uri(config, root_owner, root);
    let descendants =
        collect_descendants_for_local_root(db, config, viewer, root, &root_uri).await?;

    Ok(MastodonContextResponse {
        ancestors,
        descendants,
    })
}

async fn collect_descendants_for_local_root(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    root: &StatusRow,
    root_uri: &str,
) -> Result<Vec<MastodonStatusResponse>> {
    let max_depth = context_descendant_max_depth(viewer.is_some());
    let mut descendants = Vec::new();
    let mut queued_local_nodes = vec![LocalContextQueueNode {
        status_id: root.id.clone(),
        object_uri: root_uri.to_owned(),
        depth: 0,
    }];
    let mut queued_remote_uris = Vec::new();
    let mut seen_local_ids = HashSet::from([root.id.clone()]);
    let mut seen_remote_ids = HashSet::new();

    while let Some(node) = queued_local_nodes.pop() {
        append_local_child_descendants(
            db,
            config,
            viewer,
            &node,
            max_depth,
            &mut seen_local_ids,
            &mut queued_local_nodes,
            &mut descendants,
        )
        .await?;
        append_remote_child_descendants(
            db,
            config,
            viewer,
            &node.object_uri,
            node.depth,
            max_depth,
            &mut seen_remote_ids,
            &mut queued_remote_uris,
            &mut descendants,
        )
        .await?;
    }

    while let Some(node) = queued_remote_uris.pop() {
        append_remote_child_descendants(
            db,
            config,
            viewer,
            &node.object_uri,
            node.depth,
            max_depth,
            &mut seen_remote_ids,
            &mut queued_remote_uris,
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

async fn append_local_child_descendants(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    node: &LocalContextQueueNode,
    max_depth: Option<usize>,
    seen_local_ids: &mut HashSet<String>,
    queued_local_nodes: &mut Vec<LocalContextQueueNode>,
    descendants: &mut Vec<ContextDescendant>,
) -> Result<()> {
    for status in list_direct_local_replies(db, &node.status_id).await? {
        if !seen_local_ids.insert(status.id.clone()) {
            continue;
        }
        let Some(child_depth) = next_context_child_depth(max_depth, node.depth) else {
            continue;
        };
        let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
            continue;
        };
        if !can_view_local_status(db, &status, viewer, &owner).await? {
            continue;
        }
        descendants.push((
            status.created_at.clone(),
            build_loaded_local_status_response(db, config, viewer, &status, &owner).await?,
        ));
        queued_local_nodes.push(LocalContextQueueNode {
            status_id: status.id.clone(),
            object_uri: local_context_object_uri(config, &owner, &status),
            depth: child_depth,
        });
    }

    Ok(())
}

async fn append_remote_child_descendants(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    object_uri: &str,
    depth: usize,
    max_depth: Option<usize>,
    seen_remote_ids: &mut HashSet<String>,
    queued_remote_uris: &mut Vec<RemoteContextQueueNode>,
    descendants: &mut Vec<ContextDescendant>,
) -> Result<()> {
    for (status, actor) in list_direct_remote_replies_by_uri(db, object_uri).await? {
        if !seen_remote_ids.insert(status.id.clone()) {
            continue;
        }
        let Some(child_depth) = next_context_child_depth(max_depth, depth) else {
            continue;
        };
        if !is_public_activitypub_visibility(status.visibility.as_str()) {
            continue;
        }
        descendants.push((
            status.published_at.clone(),
            build_remote_status_response(db, config, viewer, &status, &actor).await?,
        ));
        queued_remote_uris.push(RemoteContextQueueNode {
            object_uri: status.object_uri.clone(),
            depth: child_depth,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_depth_exceeds_limit_only_after_limit() {
        assert!(!context_depth_exceeds_limit(None, usize::MAX));
        assert!(!context_depth_exceeds_limit(Some(2), 2));
        assert!(context_depth_exceeds_limit(Some(2), 3));
    }

    #[test]
    fn next_context_child_depth_respects_limit() {
        assert_eq!(next_context_child_depth(None, usize::MAX), Some(usize::MAX));
        assert_eq!(next_context_child_depth(Some(2), 1), Some(2));
        assert_eq!(next_context_child_depth(Some(2), 2), None);
    }
}
