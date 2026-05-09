use super::{
    AppConfig, D1Database, LocalAccount, MastodonContextResponse, MastodonStatusResponse,
    StatusRow, actor_url, build_local_status_response, build_remote_status_response,
    can_view_local_status, context_descendant_max_depth, find_account_by_id,
    find_media_attachments_by_status_id, find_status_by_id, is_public_activitypub_visibility,
    list_direct_local_replies, list_direct_remote_replies_by_uri, load_in_reply_to_account_id,
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
            actor_url(config, &owner.username),
            status.id
        )
    })
}

fn context_depth_exceeds_limit(max_depth: Option<usize>, depth: usize) -> bool {
    max_depth.is_some_and(|limit| depth > limit)
}

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
        let media = find_media_attachments_by_status_id(db, &status.id).await?;
        let in_reply_to_account_id = load_in_reply_to_account_id(db, &status).await?;
        ancestors.push(
            build_local_status_response(
                db,
                config,
                viewer,
                &status,
                &owner,
                in_reply_to_account_id,
                media,
            )
            .await?,
        );
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
    let mut queued_local_nodes = vec![(root.id.clone(), root_uri.to_owned(), 0usize)];
    let mut queued_remote_uris = Vec::new();
    let mut seen_local_ids = HashSet::from([root.id.clone()]);
    let mut seen_remote_ids = HashSet::new();

    while let Some((status_id, object_uri, depth)) = queued_local_nodes.pop() {
        for status in list_direct_local_replies(db, &status_id).await? {
            if !seen_local_ids.insert(status.id.clone()) {
                continue;
            }
            let child_depth = depth.saturating_add(1);
            if context_depth_exceeds_limit(max_depth, child_depth) {
                continue;
            }
            let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
                continue;
            };
            if !can_view_local_status(db, &status, viewer, &owner).await? {
                continue;
            }
            let media = find_media_attachments_by_status_id(db, &status.id).await?;
            let in_reply_to_account_id = load_in_reply_to_account_id(db, &status).await?;
            descendants.push((
                status.created_at.clone(),
                build_local_status_response(
                    db,
                    config,
                    viewer,
                    &status,
                    &owner,
                    in_reply_to_account_id,
                    media,
                )
                .await?,
            ));
            queued_local_nodes.push((
                status.id.clone(),
                local_context_object_uri(config, &owner, &status),
                child_depth,
            ));
        }
        for (status, actor) in list_direct_remote_replies_by_uri(db, &object_uri).await? {
            if !seen_remote_ids.insert(status.id.clone()) {
                continue;
            }
            let child_depth = depth.saturating_add(1);
            if context_depth_exceeds_limit(max_depth, child_depth) {
                continue;
            }
            if !is_public_activitypub_visibility(&status.visibility) {
                continue;
            }
            descendants.push((
                status.published_at.clone(),
                build_remote_status_response(db, config, viewer, &status, &actor).await?,
            ));
            queued_remote_uris.push((status.object_uri.clone(), child_depth));
        }
    }

    while let Some((object_uri, depth)) = queued_remote_uris.pop() {
        for (status, actor) in list_direct_remote_replies_by_uri(db, &object_uri).await? {
            if !seen_remote_ids.insert(status.id.clone()) {
                continue;
            }
            let child_depth = depth.saturating_add(1);
            if context_depth_exceeds_limit(max_depth, child_depth) {
                continue;
            }
            if !is_public_activitypub_visibility(&status.visibility) {
                continue;
            }
            descendants.push((
                status.published_at.clone(),
                build_remote_status_response(db, config, viewer, &status, &actor).await?,
            ));
            queued_remote_uris.push((status.object_uri.clone(), child_depth));
        }
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
}
