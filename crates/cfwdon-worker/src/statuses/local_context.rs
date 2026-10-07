use super::{
    LocalAccount, can_view_local_status, list_direct_local_replies_to_ids,
    list_direct_remote_replies_by_uris, list_local_reply_chain,
};
use crate::accounts::find_accounts_by_ids;
use crate::activitypub::is_public_activitypub_visibility;
use crate::identity::actor_url;
use crate::response::{
    MastodonContextResponse, context_descendant_max_depth, trim_context_ancestors,
    trim_context_descendants,
};
use crate::timelines::{StatusRenderItem, render_status_items};
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalStatus;
use std::collections::{HashMap, HashSet};
use worker::Result;
fn local_context_object_uri(
    config: &AppConfig,
    owner: &LocalAccount,
    status: &LocalStatus,
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

/// How far up a reply chain the ancestor query follows; ancestors are trimmed
/// further for the response.
const MAX_CONTEXT_ANCESTOR_DEPTH: u32 = 256;

fn unique_account_ids(statuses: &[LocalStatus]) -> Vec<String> {
    let mut seen = HashSet::new();
    statuses
        .iter()
        .map(|status| status.account_id.clone())
        .filter(|account_id| seen.insert(account_id.clone()))
        .collect()
}

/// Render ancestors and descendants in one batched pass.
pub(super) async fn render_context_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    ancestors: Vec<StatusRenderItem>,
    descendants: Vec<StatusRenderItem>,
) -> Result<MastodonContextResponse> {
    let ancestor_count = ancestors.len();
    let mut items = ancestors;
    items.extend(descendants);
    let mut rendered = render_status_items(db, config, viewer, items).await?;
    let descendants = rendered.split_off(ancestor_count.min(rendered.len()));
    Ok(MastodonContextResponse {
        ancestors: rendered,
        descendants,
    })
}

pub(crate) async fn build_local_status_context(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    root: &LocalStatus,
    root_owner: &LocalAccount,
) -> Result<MastodonContextResponse> {
    let is_authenticated = viewer.is_some();
    let mut ancestors = Vec::new();
    if let Some(parent_id) = root.in_reply_to_id.as_deref() {
        let chain = list_local_reply_chain(db, parent_id, MAX_CONTEXT_ANCESTOR_DEPTH).await?;
        let owners = find_accounts_by_ids(db, &unique_account_ids(&chain)).await?;
        let mut seen_local_ids = HashSet::new();
        for status in chain {
            if !seen_local_ids.insert(status.id.clone()) {
                break;
            }
            let Some(owner) = owners.get(&status.account_id) else {
                break;
            };
            if !can_view_local_status(db, &status, viewer, owner).await? {
                break;
            }
            ancestors.push(StatusRenderItem::Local(status));
        }
    }
    ancestors.reverse();
    let ancestors = trim_context_ancestors(ancestors, is_authenticated);

    let root_uri = local_context_object_uri(config, root_owner, root);
    let descendants =
        collect_descendants_for_local_root(db, config, viewer, root, &root_uri).await?;

    render_context_response(db, config, viewer, ancestors, descendants).await
}

/// Walks the reply tree a level at a time: each level's local and remote
/// replies are one query each, whatever the number of nodes.
async fn collect_descendants_for_local_root(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    root: &LocalStatus,
    root_uri: &str,
) -> Result<Vec<StatusRenderItem>> {
    let max_depth = context_descendant_max_depth(viewer.is_some());
    let mut descendants = Vec::new();
    let mut local_level = vec![LocalContextQueueNode {
        status_id: root.id.clone(),
        object_uri: root_uri.to_owned(),
        depth: 0,
    }];
    let mut remote_level = Vec::<RemoteContextQueueNode>::new();
    let mut seen_local_ids = HashSet::from([root.id.clone()]);
    let mut seen_remote_ids = HashSet::new();

    while !local_level.is_empty() || !remote_level.is_empty() {
        let local_parent_depths = local_level
            .iter()
            .map(|node| (node.status_id.clone(), node.depth))
            .collect::<HashMap<_, _>>();
        let uri_parent_depths = local_level
            .iter()
            .map(|node| (node.object_uri.clone(), node.depth))
            .chain(
                remote_level
                    .iter()
                    .map(|node| (node.object_uri.clone(), node.depth)),
            )
            .collect::<HashMap<_, _>>();
        let local_parent_ids = local_parent_depths.keys().cloned().collect::<Vec<_>>();
        let parent_uris = uri_parent_depths.keys().cloned().collect::<Vec<_>>();
        let (local_replies, remote_replies) = futures_util::try_join!(
            list_direct_local_replies_to_ids(db, &local_parent_ids),
            list_direct_remote_replies_by_uris(db, &parent_uris),
        )?;
        let owners = find_accounts_by_ids(db, &unique_account_ids(&local_replies)).await?;

        let mut next_local_level = Vec::new();
        for status in local_replies {
            let Some(parent_depth) = status
                .in_reply_to_id
                .as_deref()
                .and_then(|parent_id| local_parent_depths.get(parent_id))
            else {
                continue;
            };
            if !seen_local_ids.insert(status.id.clone()) {
                continue;
            }
            let Some(child_depth) = next_context_child_depth(max_depth, *parent_depth) else {
                continue;
            };
            let Some(owner) = owners.get(&status.account_id) else {
                continue;
            };
            if !can_view_local_status(db, &status, viewer, owner).await? {
                continue;
            }
            next_local_level.push(LocalContextQueueNode {
                status_id: status.id.clone(),
                object_uri: local_context_object_uri(config, owner, &status),
                depth: child_depth,
            });
            descendants.push((status.created_at.clone(), StatusRenderItem::Local(status)));
        }

        let mut next_remote_level = Vec::new();
        for (status, actor) in remote_replies {
            let Some(parent_depth) = status
                .in_reply_to_uri
                .as_deref()
                .and_then(|parent_uri| uri_parent_depths.get(parent_uri))
            else {
                continue;
            };
            if !seen_remote_ids.insert(status.id.clone()) {
                continue;
            }
            let Some(child_depth) = next_context_child_depth(max_depth, *parent_depth) else {
                continue;
            };
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
                continue;
            }
            next_remote_level.push(RemoteContextQueueNode {
                object_uri: status.object_uri.clone(),
                depth: child_depth,
            });
            descendants.push((
                status.published_at.clone(),
                StatusRenderItem::Remote { status, actor },
            ));
        }

        local_level = next_local_level;
        remote_level = next_remote_level;
    }

    descendants.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(trim_context_descendants(
        descendants.into_iter().map(|(_, status)| status).collect(),
        viewer.is_some(),
    ))
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
