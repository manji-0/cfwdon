use super::{
    AppConfig, Env, LocalAccount, LocalStatusResponsePreload, MastodonStatusResponse,
    MediaAttachmentRow, Result, StatusRow, attach_media_and_enqueue_outbox,
    build_local_status_response_for_recipient_soft,
    build_local_status_response_with_quote_count_preloads, ensure_direct_conversation_for_status,
    extract_mentions_from_text, find_account_by_username, find_local_status_by_object_uri,
    insert_status, load_account_filter_matcher, load_local_status_response_preload,
    local_status_interaction_notification_id, preload_local_status_viewer_state,
    preload_mastodon_poll_responses, preload_status_counts, preload_status_quote_counts,
    publish_local_actor_notification_soft, publish_local_status_create_stream_fanout_soft,
    publish_user_stream_hub_event_soft, send_push_notification, send_status_quote_notification,
};
use cfwdon_domain::{QuoteState, StatusDraft};
use worker::console_error;

use crate::D1Database;

pub(crate) struct CreatePublishedStatusInput<'a> {
    pub(crate) account: &'a LocalAccount,
    pub(crate) application_id: Option<i64>,
    pub(crate) draft: &'a StatusDraft,
    pub(crate) pending_media: &'a [MediaAttachmentRow],
    pub(crate) in_reply_to_account_id: Option<String>,
    pub(crate) quote_of_uri: Option<&'a str>,
}

struct PublishedStatusArtifacts {
    response: MastodonStatusResponse,
    status: StatusRow,
    response_preload: LocalStatusResponsePreload,
    counts_preload: crate::StatusCountsPreload,
    quote_counts_preload: crate::StatusQuoteCountsPreload,
    has_media: bool,
}

/// Builds the stream payload used for fan-out to accounts other than the
/// author. Viewer-dependent fields are left unset so no per-account state
/// (filters, favourites, bookmarks) leaks to other subscribers.
pub(crate) async fn viewer_agnostic_local_status_stream_payload(
    db: &D1Database,
    config: &AppConfig,
    status: &StatusRow,
    author: &LocalAccount,
    response_preload: &LocalStatusResponsePreload,
    counts_preload: &crate::StatusCountsPreload,
    quote_counts_preload: &crate::StatusQuoteCountsPreload,
) -> Option<String> {
    let response = match build_local_status_response_with_quote_count_preloads(
        db,
        config,
        None,
        status,
        author,
        response_preload.in_reply_to_account_id.clone(),
        response_preload.media.clone(),
        None,
        Some(counts_preload),
        Some(quote_counts_preload),
        None,
        None,
        None,
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            console_error!(
                "failed to build viewer-agnostic stream payload for status {}: {error}",
                status.id
            );
            return None;
        }
    };
    match serde_json::to_string(&response) {
        Ok(payload) => Some(payload),
        Err(error) => {
            console_error!(
                "failed to serialize viewer-agnostic stream payload for status {}: {error}",
                status.id
            );
            None
        }
    }
}

async fn send_create_status_push_notifications(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
    in_reply_to_account_id: Option<&str>,
) {
    let _ = send_status_quote_notification(db, config, status).await;
    if let Some(recipient_account_id) = in_reply_to_account_id
        && recipient_account_id != account.id()
    {
        let _ = send_push_notification(
            db,
            config,
            recipient_account_id,
            "status",
            serde_json::json!({
                "account_id": account.id(),
                "status_id": status.id,
                "in_reply_to_account_id": recipient_account_id,
            }),
        )
        .await;
    }
    for handle in extract_mentions_from_text(&status.text, config) {
        if let Ok(Some(mentioned)) = find_account_by_username(db, &handle.username).await
            && mentioned.id() != account.id()
        {
            let _ = send_push_notification(
                db,
                config,
                mentioned.id(),
                "mention",
                serde_json::json!({
                    "account_id": account.id(),
                    "status_id": status.id,
                }),
            )
            .await;
        }
    }
}

async fn build_published_status_artifacts(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<PublishedStatusArtifacts> {
    let response_preload = load_local_status_response_preload(db, status).await?;
    let has_media = !response_preload.media.is_empty();
    let status_ids = vec![status.id.clone()];
    let quote_count_uris = vec![crate::local_status_ap_id(config, account, status)];
    let status_refs = vec![status];
    let (counts_preload, quote_counts_preload, poll_preload, viewer_state_preload, filter_matcher) =
        futures_util::try_join!(
            preload_status_counts(db, &status_ids, &[]),
            preload_status_quote_counts(db, &quote_count_uris),
            preload_mastodon_poll_responses(db, &status_ids, Some(account)),
            preload_local_status_viewer_state(db, account.id(), &status_refs, None),
            load_account_filter_matcher(db, account.id()),
        )?;
    let response = build_local_status_response_with_quote_count_preloads(
        db,
        config,
        Some(account),
        status,
        account,
        response_preload.in_reply_to_account_id.clone(),
        response_preload.media.clone(),
        Some(&filter_matcher),
        Some(&counts_preload),
        Some(&quote_counts_preload),
        Some(&poll_preload),
        Some(&viewer_state_preload),
        None,
    )
    .await?;

    Ok(PublishedStatusArtifacts {
        response,
        status: status.clone(),
        response_preload,
        counts_preload,
        quote_counts_preload,
        has_media,
    })
}

async fn publish_created_status_stream_events(
    env: &Env,
    db: &D1Database,
    config: &AppConfig,
    input: &CreatePublishedStatusInput<'_>,
    artifacts: &PublishedStatusArtifacts,
) -> MastodonStatusResponse {
    let status = &artifacts.status;
    let payload = match serde_json::to_string(&artifacts.response) {
        Ok(payload) => payload,
        Err(error) => {
            console_error!(
                "failed to serialize status create stream payload for status {}: {error}",
                status.id
            );
            return artifacts.response.clone();
        }
    };
    publish_user_stream_hub_event_soft(
        env,
        &config.stream_hub_binding,
        input.account.id(),
        "update",
        &payload,
        Some(&status.id),
    )
    .await;

    let fanout_payload = match viewer_agnostic_local_status_stream_payload(
        db,
        config,
        status,
        input.account,
        &artifacts.response_preload,
        &artifacts.counts_preload,
        &artifacts.quote_counts_preload,
    )
    .await
    {
        Some(payload) => payload,
        None => return artifacts.response.clone(),
    };

    publish_local_status_create_stream_fanout_soft(
        env,
        db,
        config,
        input.account,
        status,
        &fanout_payload,
        artifacts.has_media,
    )
    .await;

    if let Some(recipient_account_id) = input.in_reply_to_account_id.as_deref()
        && recipient_account_id != input.account.id()
    {
        let id = local_status_interaction_notification_id("status", input.account.id(), &status.id);
        let status_response = build_local_status_response_for_recipient_soft(
            db,
            config,
            recipient_account_id,
            status,
            input.account,
        )
        .await;
        publish_local_actor_notification_soft(
            Some(env),
            db,
            config,
            recipient_account_id,
            input.account,
            "status",
            id.clone(),
            id,
            status.created_at.clone(),
            status_response,
        )
        .await;
    }

    for handle in extract_mentions_from_text(&status.text, config) {
        if let Ok(Some(account)) = find_account_by_username(db, &handle.username).await
            && account.id() != input.account.id()
        {
            let id =
                local_status_interaction_notification_id("mention", input.account.id(), &status.id);
            let status_response = build_local_status_response_for_recipient_soft(
                db,
                config,
                account.id(),
                status,
                input.account,
            )
            .await;
            publish_local_actor_notification_soft(
                Some(env),
                db,
                config,
                account.id(),
                input.account,
                "mention",
                id.clone(),
                id,
                status.created_at.clone(),
                status_response,
            )
            .await;
        }
    }

    if let Some(quote_of_uri) = status.quote_of_uri.as_deref()
        && status.quote_state == QuoteState::Accepted
        && let Ok(Some(target)) = find_local_status_by_object_uri(db, config, quote_of_uri).await
        && target.account_id != input.account.id()
    {
        let id = local_status_interaction_notification_id("quote", input.account.id(), &status.id);
        let status_response = build_local_status_response_for_recipient_soft(
            db,
            config,
            &target.account_id,
            status,
            input.account,
        )
        .await;
        publish_local_actor_notification_soft(
            Some(env),
            db,
            config,
            &target.account_id,
            input.account,
            "quote",
            id.clone(),
            id,
            status.created_at.clone(),
            status_response,
        )
        .await;
    }

    artifacts.response.clone()
}

pub(crate) async fn create_published_status_and_response(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
    input: CreatePublishedStatusInput<'_>,
) -> Result<MastodonStatusResponse> {
    let defer_outbox = !input.pending_media.is_empty();
    let status = insert_status(
        db,
        config,
        input.account,
        input.draft,
        input.application_id,
        input.quote_of_uri,
        defer_outbox,
        input.in_reply_to_account_id.clone(),
    )
    .await?;
    ensure_direct_conversation_for_status(db, config, input.account, input.draft, &status).await?;
    if !input.pending_media.is_empty() {
        attach_media_and_enqueue_outbox(db, config, input.account, &status, input.pending_media)
            .await?;
    }
    send_create_status_push_notifications(
        db,
        config,
        input.account,
        &status,
        input.in_reply_to_account_id.as_deref(),
    )
    .await;
    let artifacts = build_published_status_artifacts(db, config, input.account, &status).await?;

    if let Some(env) = env {
        return Ok(publish_created_status_stream_events(env, db, config, &input, &artifacts).await);
    }

    Ok(artifacts.response)
}
