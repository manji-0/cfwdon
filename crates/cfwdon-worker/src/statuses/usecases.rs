use super::{
    AppConfig, D1Database, LocalAccount, LocalStatusResponsePreload, MastodonStatusResponse,
    MediaAttachmentRow, Result, StatusMediaAttributeRequest, StatusRow, UpdateMediaRequest,
    apply_media_update, attach_media_to_status, build_local_status_response,
    build_local_status_response_with_quote_count_preloads, delete_status_by_id,
    enqueue_outbox_activity, enqueue_outbox_delete, enqueue_status_update_activity,
    ensure_direct_conversation_for_status, extract_mentions_from_text, find_account_by_username,
    find_media_attachments_by_status_id, find_owned_local_status, insert_status,
    insert_status_edit_snapshot, load_account_filter_matcher, load_local_status_response_preload,
    load_mastodon_poll_response, normalize_status_history_entry, now_iso_string,
    preload_local_status_viewer_state, preload_mastodon_poll_responses, preload_status_counts,
    preload_status_quote_counts, replace_status_media, replace_status_poll, send_push_notification,
    send_status_quote_notification, send_status_update_notifications, update_local_status,
};
use cfwdon_domain::{PollDraft, StatusDraft};

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

pub(crate) struct CreatePublishedStatusInput<'a> {
    pub(crate) account: &'a LocalAccount,
    pub(crate) application_id: Option<i64>,
    pub(crate) draft: &'a StatusDraft,
    pub(crate) pending_media: &'a [MediaAttachmentRow],
    pub(crate) in_reply_to_account_id: Option<String>,
    pub(crate) quote_of_uri: Option<&'a str>,
}

pub(crate) async fn delete_owned_local_status(
    db: &D1Database,
    config: &AppConfig,
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

    enqueue_outbox_delete(db, config, requester, &status).await?;
    delete_status_by_id(db, &status.id).await?;

    Ok(Some(DeleteLocalStatusResult {
        response,
        media,
        status_id: status.id,
    }))
}

pub(crate) async fn apply_local_status_update(
    db: &D1Database,
    config: &AppConfig,
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
    let _ = send_status_update_notifications(db, config, &status).await;

    let response = super::build_loaded_local_status_response(
        db,
        config,
        Some(input.account),
        &status,
        input.account,
    )
    .await?;

    Ok(UpdateLocalStatusResult {
        response,
        status_id: status.id,
    })
}

pub(crate) async fn create_published_status_and_response(
    db: &D1Database,
    config: &AppConfig,
    input: CreatePublishedStatusInput<'_>,
) -> Result<MastodonStatusResponse> {
    let status = insert_status(
        db,
        config,
        input.account,
        input.draft,
        input.application_id,
        input.quote_of_uri,
    )
    .await?;
    ensure_direct_conversation_for_status(db, config, input.account, input.draft, &status).await?;
    attach_media_to_status(db, &status.id, input.pending_media).await?;
    let response_preload = load_local_status_response_preload(db, &status).await?;
    enqueue_outbox_activity(db, config, input.account, &status).await?;
    let _ = send_status_quote_notification(db, config, &status).await;
    if let Some(recipient_account_id) = input.in_reply_to_account_id.as_deref()
        && recipient_account_id != input.account.id()
    {
        let _ = send_push_notification(
            db,
            config,
            recipient_account_id,
            "status",
            serde_json::json!({
                "account_id": input.account.id(),
                "status_id": status.id,
                "in_reply_to_account_id": recipient_account_id,
            }),
        )
        .await;
    }
    for handle in extract_mentions_from_text(&status.text, config) {
        if let Some(account) = find_account_by_username(db, &handle.username).await?
            && account.id() != input.account.id()
        {
            let _ = send_push_notification(
                db,
                config,
                account.id(),
                "mention",
                serde_json::json!({
                    "account_id": input.account.id(),
                    "status_id": status.id,
                }),
            )
            .await;
        }
    }
    let status_ids = vec![status.id.clone()];
    let quote_count_uris = vec![crate::local_status_ap_id(config, input.account, &status)];
    let status_refs = vec![&status];
    let (counts_preload, quote_counts_preload, poll_preload, viewer_state_preload, filter_matcher) =
        futures_util::try_join!(
            preload_status_counts(db, &status_ids, &[]),
            preload_status_quote_counts(db, &quote_count_uris),
            preload_mastodon_poll_responses(db, &status_ids, Some(input.account)),
            preload_local_status_viewer_state(db, input.account.id(), &status_refs, None),
            load_account_filter_matcher(db, input.account.id()),
        )?;
    build_local_status_response_with_quote_count_preloads(
        db,
        config,
        Some(input.account),
        &status,
        input.account,
        response_preload.in_reply_to_account_id,
        response_preload.media,
        Some(&filter_matcher),
        Some(&counts_preload),
        Some(&quote_counts_preload),
        Some(&poll_preload),
        Some(&viewer_state_preload),
        None,
    )
    .await
}
