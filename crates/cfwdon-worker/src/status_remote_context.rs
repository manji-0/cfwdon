use super::{
    AppConfig, D1Database, MastodonContextResponse, MastodonStatusResponse, RemoteActorRow,
    RemoteStatusRow, build_local_status_response, build_remote_status_response, find_account_by_id,
    find_media_attachments_by_status_id, find_remote_actor_by_actor_uri,
    find_remote_status_by_object_uri, find_status_by_ap_id, is_public_activitypub_visibility,
    list_direct_remote_replies_by_uri, load_in_reply_to_account_id,
};
use std::collections::HashSet;
use worker::Result;

pub(crate) async fn build_remote_status_context(
    db: &D1Database,
    config: &AppConfig,
    root: &RemoteStatusRow,
    root_actor: &RemoteActorRow,
) -> Result<MastodonContextResponse> {
    let mut ancestors = Vec::new();
    let mut current = root.in_reply_to_uri.clone();
    let mut seen_remote_ids = HashSet::new();

    while let Some(object_uri) = current {
        if let Some(local_status) = find_status_by_ap_id(db, &object_uri).await? {
            let Some(owner) = find_account_by_id(db, &local_status.account_id).await? else {
                break;
            };
            if !is_public_activitypub_visibility(&local_status.visibility) {
                break;
            }
            let media = find_media_attachments_by_status_id(db, &local_status.id).await?;
            let in_reply_to_account_id = load_in_reply_to_account_id(db, &local_status).await?;
            ancestors.push(
                build_local_status_response(
                    db,
                    config,
                    None,
                    &local_status,
                    &owner,
                    in_reply_to_account_id,
                    media,
                )
                .await?,
            );
            break;
        }

        let Some(status) = find_remote_status_by_object_uri(db, &object_uri).await? else {
            break;
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
        ancestors.push(build_remote_status_response(db, config, None, &status, &actor).await?);
        current = status.in_reply_to_uri.clone();
    }
    ancestors.reverse();

    let descendants = collect_descendants_for_remote_root(db, config, root, root_actor).await?;
    Ok(MastodonContextResponse {
        ancestors,
        descendants,
    })
}

async fn collect_descendants_for_remote_root(
    db: &D1Database,
    config: &AppConfig,
    root: &RemoteStatusRow,
    _root_actor: &RemoteActorRow,
) -> Result<Vec<MastodonStatusResponse>> {
    let mut descendants = Vec::new();
    let mut queued_uris = vec![root.object_uri.clone()];
    let mut seen_remote_ids = HashSet::from([root.id.clone()]);

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
                build_remote_status_response(db, config, None, &status, &actor).await?,
            ));
            queued_uris.push(status.object_uri.clone());
        }
    }

    descendants.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(descendants.into_iter().map(|(_, status)| status).collect())
}
