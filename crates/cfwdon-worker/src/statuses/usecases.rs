use super::{
    AppConfig, Env, LocalAccount, LocalStatusResponsePreload, MastodonStatusResponse,
    MediaAttachmentRow, Result, StatusMediaAttributeRequest, StatusRow, UpdateMediaRequest,
    apply_media_update, build_local_status_response, delete_local_status_with_outbox,
    enqueue_status_update_activity, find_media_attachments_by_status_id, find_owned_local_status,
    insert_status_edit_snapshot, load_local_status_response_preload, load_mastodon_poll_response,
    normalize_status_history_entry, now_iso_string, preload_status_counts,
    preload_status_quote_counts, publish_local_status_delete_stream_fanout_soft,
    publish_local_status_update_stream_fanout_soft, publish_user_stream_hub_event_soft,
    replace_status_media, replace_status_poll, send_status_update_notifications,
    update_local_status,
};
use cfwdon_domain::PollDraft;
use worker::console_error;

use crate::D1Database;

pub(crate) use super::create_published::viewer_agnostic_local_status_stream_payload;

pub(crate) struct DeleteLocalStatusResult {
    pub(crate) response: MastodonStatusResponse,
    pub(crate) media: Vec<MediaAttachmentRow>,
    pub(crate) status_id: String,
}

pub(crate) struct UpdateLocalStatusInput<'a> {
    pub(crate) account: &'a LocalAccount,
    pub(crate) status: &'a StatusRow,
    pub(crate) current_media: Vec<MediaAttachmentRow>,
    pub(crate) current_in_reply_to_account_id: Option<String>,
    pub(crate) next_text: &'a str,
    pub(crate) next_spoiler_text: &'a str,
    pub(crate) next_sensitive: bool,
    pub(crate) next_language: Option<&'a str>,
    pub(crate) next_poll: Option<&'a PollDraft>,
    pub(crate) requested_media: Option<&'a [MediaAttachmentRow]>,
    pub(crate) media_attributes: Option<&'a [StatusMediaAttributeRequest]>,
}

pub(crate) struct UpdateLocalStatusResult {
    pub(crate) response: MastodonStatusResponse,
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

pub(crate) async fn apply_local_status_update(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
    input: UpdateLocalStatusInput<'_>,
) -> Result<UpdateLocalStatusResult> {
    let previous_response = build_local_status_response(
        db,
        config,
        Some(input.account),
        input.status,
        input.account,
        input.current_in_reply_to_account_id.clone(),
        input.current_media.clone(),
    )
    .await?;
    let mut previous_snapshot =
        serde_json::to_value(previous_response).unwrap_or_else(|_| serde_json::json!({}));
    let revision_at = now_iso_string()?;
    previous_snapshot["created_at"] = serde_json::json!(revision_at.clone());
    let previous_snapshot = normalize_status_history_entry(previous_snapshot);
    let previous_snapshot_json = serde_json::to_string(&previous_snapshot).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize status snapshot: {error}"))
    })?;
    insert_status_edit_snapshot(db, &input.status.id, &previous_snapshot_json, &revision_at)
        .await?;

    let status = update_local_status(
        db,
        config,
        input.status,
        input.next_text,
        input.next_spoiler_text,
        input.next_sensitive,
        input.next_language,
        &revision_at,
    )
    .await?;
    if let Some(media) = input.requested_media {
        replace_status_media(db, &status.id, media).await?;
    }
    if let Some(poll) = input.next_poll {
        replace_status_poll(db, &status.id, poll, &revision_at).await?;
    }
    if let Some(attributes) = input.media_attributes {
        let attached_media = find_media_attachments_by_status_id(db, &status.id).await?;
        for (index, attribute) in attributes.iter().enumerate() {
            let target_id = attribute
                .id
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .or_else(|| attached_media.get(index).map(|media| media.id.clone()));
            let Some(target_id) = target_id else {
                continue;
            };
            let Some(media) = attached_media.iter().find(|media| media.id == target_id) else {
                return Err(worker::Error::RustError(format!(
                    "unknown media attachment in media_attributes: {target_id}"
                )));
            };
            apply_media_update(
                db,
                media,
                UpdateMediaRequest {
                    description: attribute.description.clone(),
                    focus: attribute.focus.clone(),
                },
            )
            .await?;
        }
    }
    enqueue_status_update_activity(db, config, input.account, &status).await?;
    let _ = send_status_update_notifications(db, config, env, &status).await;

    let response = super::build_loaded_local_status_response(
        db,
        config,
        Some(input.account),
        &status,
        input.account,
    )
    .await?;

    if let Some(env) = env {
        let payload = match serde_json::to_string(&response) {
            Ok(payload) => payload,
            Err(error) => {
                console_error!(
                    "failed to serialize status update stream payload for status {}: {error}",
                    status.id
                );
                return Ok(UpdateLocalStatusResult {
                    response,
                    status_id: status.id,
                });
            }
        };
        publish_user_stream_hub_event_soft(
            env,
            &config.stream_hub_binding,
            input.account.id(),
            "status.update",
            &payload,
            Some(&status.id),
        )
        .await;

        // Timeline subscribers need the edit too, with a viewer-agnostic payload.
        let response_preload = load_local_status_response_preload(db, &status).await?;
        let has_media = !response_preload.media.is_empty();
        let status_ids = vec![status.id.clone()];
        let quote_count_uris = vec![crate::local_status_ap_id(config, input.account, &status)];
        let (counts_preload, quote_counts_preload) = futures_util::try_join!(
            preload_status_counts(db, &status_ids, &[]),
            preload_status_quote_counts(db, &quote_count_uris),
        )?;
        if let Some(fanout_payload) = viewer_agnostic_local_status_stream_payload(
            db,
            config,
            &status,
            input.account,
            &response_preload,
            &counts_preload,
            &quote_counts_preload,
        )
        .await
        {
            publish_local_status_update_stream_fanout_soft(
                env,
                db,
                config,
                input.account,
                &status,
                &fanout_payload,
                has_media,
            )
            .await;
        }
    }

    Ok(UpdateLocalStatusResult {
        response,
        status_id: status.id,
    })
}
