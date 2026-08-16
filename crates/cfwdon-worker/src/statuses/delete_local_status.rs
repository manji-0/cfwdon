use super::{
    AppConfig, Env, LocalAccount, LocalStatusResponsePreload, MastodonStatusResponse,
    MediaAttachmentRow, Result, delete_local_status_with_outbox, find_owned_local_status,
    load_local_status_response_preload, load_mastodon_poll_response,
    publish_local_status_delete_stream_fanout_soft, publish_user_stream_hub_event_soft,
};

use crate::D1Database;

pub(crate) struct DeleteLocalStatusResult {
    pub(crate) response: MastodonStatusResponse,
    pub(crate) media: Vec<MediaAttachmentRow>,
    pub(crate) status_id: String,
}

pub(crate) async fn delete_owned_local_status(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
    requester: &LocalAccount,
    status_id: &str,
) -> Result<Option<DeleteLocalStatusResult>> {
    let Some(status) = find_owned_local_status(db, status_id, requester.id()).await? else {
        return Ok(None);
    };

    let LocalStatusResponsePreload {
        media,
        in_reply_to_account_id,
    } = load_local_status_response_preload(db, &status).await?;
    let mut response = MastodonStatusResponse::from_deleted_row(
        &status,
        requester,
        config,
        in_reply_to_account_id,
        media.clone(),
    );
    response.poll = load_mastodon_poll_response(db, &status.id, Some(requester)).await?;

    delete_local_status_with_outbox(db, config, requester, &status).await?;

    if let Some(env) = env {
        publish_user_stream_hub_event_soft(
            env,
            &config.stream_hub_binding,
            requester.id(),
            "delete",
            &status.id,
            Some(&status.id),
        )
        .await;

        publish_local_status_delete_stream_fanout_soft(
            env,
            db,
            config,
            requester,
            &status,
            !media.is_empty(),
        )
        .await;
    }

    Ok(Some(DeleteLocalStatusResult {
        response,
        media,
        status_id: status.id,
    }))
}
