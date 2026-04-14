use super::{
    AppConfig, D1Database, LocalAccount, MastodonContextResponse, MastodonStatusResponse,
    StatusRow, actor_url, build_local_status_response, build_remote_status_response,
    can_view_local_status, find_account_by_id, find_media_attachments_by_status_id,
    find_status_by_id, is_public_activitypub_visibility, list_direct_local_replies,
    list_direct_remote_replies_by_uri, load_in_reply_to_account_id,
};
use std::collections::HashSet;
use worker::Result;

pub(crate) async fn build_local_status_context(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    root: &StatusRow,
    root_owner: &LocalAccount,
) -> Result<MastodonContextResponse> {
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

    let root_uri = root.ap_id.clone().unwrap_or_else(|| {
        format!(
            "{}/statuses/{}",
            actor_url(config, &root_owner.username),
            root.id
        )
    });
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
    let mut descendants = Vec::new();
    let mut queued_local_ids = vec![root.id.clone()];
    let mut queued_uris = vec![root_uri.to_owned()];
    let mut seen_local_ids = HashSet::new();
    let mut seen_remote_ids = HashSet::new();

    while let Some(status_id) = queued_local_ids.pop() {
        if !seen_local_ids.insert(status_id.clone()) {
            continue;
        }
        for status in list_direct_local_replies(db, &status_id).await? {
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
            queued_local_ids.push(status.id.clone());
        }
    }

    while let Some(object_uri) = queued_uris.pop() {
        for (status, actor) in list_direct_remote_replies_by_uri(db, &object_uri).await? {
            if !seen_remote_ids.insert(status.id.clone()) {
                continue;
            }
            if !is_public_activitypub_visibility(&status.visibility) {
                continue;
            }
            descendants.push((
                status.published_at.clone(),
                build_remote_status_response(db, config, viewer, &status, &actor).await?,
            ));
            queued_uris.push(status.object_uri.clone());
        }
    }

    descendants.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(descendants.into_iter().map(|(_, status)| status).collect())
}
