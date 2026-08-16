use super::super::{
    AccountFilterMatcher, AppConfig, BoostTarget, BoostTargetPreload, LocalAccount,
    MastodonStatusResponse, RemoteStatusRow, StatusCountsPreload, StatusRow,
    build_remote_status_card_value, build_status_card_value, build_status_mentions,
    find_local_status_by_object_uri, find_remote_actor_by_actor_uri,
    find_remote_status_attachments_by_status_id, find_remote_status_by_url_or_object_uri,
    load_mastodon_poll_response, load_remote_mastodon_poll_response,
    local_quoted_status_document_state, pending_quote_document, quote_document_for_local_state,
    quote_document_from_response, remote_quote_visibility_is_embeddable,
    remote_quoted_status_document_state, resolve_local_status_response_subject,
    unauthorized_quote_document,
};
use worker::Result;

use crate::D1Database;

pub(super) async fn build_quoted_status_value(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    quote_of_uri: Option<&str>,
    local_quote_state: Option<&str>,
    pending_remote_quote: bool,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
) -> Result<Option<serde_json::Value>> {
    let Some(quote_of_uri) = quote_of_uri else {
        return Ok(None);
    };
    if let Some(document) = quote_document_for_local_state(local_quote_state) {
        return Ok(Some(document));
    }

    if let Some(resolved) = boost_target_preload.and_then(|targets| targets.target(quote_of_uri)) {
        match resolved {
            Some(BoostTarget::Local(local_status)) => {
                return build_local_quoted_status_from_row(
                    db,
                    config,
                    viewer,
                    local_status.clone(),
                    filter_matcher,
                    counts_preload,
                )
                .await;
            }
            Some(BoostTarget::Remote(remote_status)) => {
                return build_remote_quoted_status_from_row(
                    db,
                    config,
                    viewer,
                    remote_status.clone(),
                    pending_remote_quote,
                    filter_matcher,
                    counts_preload,
                )
                .await;
            }
            None => return Ok(None),
        }
    }

    if let Some(document) = build_local_quoted_status_document(
        db,
        config,
        viewer,
        quote_of_uri,
        filter_matcher,
        counts_preload,
    )
    .await?
    {
        return Ok(Some(document));
    }

    build_remote_quoted_status_document(
        db,
        config,
        viewer,
        quote_of_uri,
        pending_remote_quote,
        filter_matcher,
        counts_preload,
    )
    .await
}

async fn build_local_quoted_status_document(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    quote_of_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
) -> Result<Option<serde_json::Value>> {
    let Some(local_status) = find_local_status_by_object_uri(db, config, quote_of_uri).await?
    else {
        return Ok(None);
    };
    build_local_quoted_status_from_row(
        db,
        config,
        viewer,
        local_status,
        filter_matcher,
        counts_preload,
    )
    .await
}

async fn build_local_quoted_status_from_row(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    local_status: StatusRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
) -> Result<Option<serde_json::Value>> {
    let Some(subject) = resolve_local_status_response_subject(db, viewer, local_status).await?
    else {
        return Ok(None);
    };
    let super::super::ResolvedLocalStatusResponseSubject::Loaded(subject) = subject else {
        return Ok(Some(unauthorized_quote_document()));
    };
    let super::super::LoadedLocalStatusResponseSubject {
        status: local_status,
        account: local_account,
        preload:
            super::super::LocalStatusResponsePreload {
                media,
                in_reply_to_account_id,
            },
    } = subject;
    let mut response = MastodonStatusResponse::from_row(
        &local_status,
        &local_account,
        config,
        in_reply_to_account_id,
        media,
    );
    response.card = build_status_card_value(&local_status.text);
    response.poll = load_mastodon_poll_response(db, &local_status.id, viewer).await?;
    response.filtered = if viewer.is_some() {
        Some(
            super::local::local_status_filtered_for_viewer(
                db,
                viewer,
                &local_status,
                filter_matcher,
            )
            .await?,
        )
    } else {
        None
    };
    response.mentions = build_status_mentions(db, config, &local_status.text).await?;
    let (favourites_count, reblogs_count) =
        super::local::local_status_counts(db, counts_preload, &local_status.id).await?;
    response.favourites_count = favourites_count;
    response.reblogs_count = reblogs_count;
    let viewer_state =
        super::local::local_status_response_viewer_state(db, viewer, &local_status, None).await?;
    let viewer_fields =
        super::local::local_viewer_interaction_fields(viewer, &local_status, viewer_state);
    response.favourited = viewer_fields.favourited;
    response.reblogged = viewer_fields.reblogged;
    response.bookmarked = viewer_fields.bookmarked;
    response.pinned = viewer_fields.pinned;
    response.muted = viewer_fields.muted;
    response.quote = None;
    let state = local_quoted_status_document_state(db, config, viewer, &local_account).await?;
    Ok(Some(quote_document_from_response(state, response)))
}

async fn build_remote_quoted_status_document(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    quote_of_uri: &str,
    pending_remote_quote: bool,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
) -> Result<Option<serde_json::Value>> {
    let Some(remote_status) = find_remote_status_by_url_or_object_uri(db, quote_of_uri).await?
    else {
        return Ok(None);
    };
    build_remote_quoted_status_from_row(
        db,
        config,
        viewer,
        remote_status,
        pending_remote_quote,
        filter_matcher,
        counts_preload,
    )
    .await
}

async fn build_remote_quoted_status_from_row(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    remote_status: RemoteStatusRow,
    pending_remote_quote: bool,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
) -> Result<Option<serde_json::Value>> {
    if pending_remote_quote {
        return Ok(Some(pending_quote_document()));
    }
    if !remote_quote_visibility_is_embeddable(remote_status.visibility.as_str()) {
        return Ok(Some(unauthorized_quote_document()));
    }
    let Some(actor) = find_remote_actor_by_actor_uri(db, &remote_status.actor_uri).await? else {
        return Ok(None);
    };
    let federated_emojis =
        super::remote::federated_emojis_for_remote_status(db, &remote_status.id, None).await?;
    let mut response = MastodonStatusResponse::from_remote_row(
        &remote_status,
        &actor,
        config,
        federated_emojis.as_ref(),
    );
    let text_content = remote_status.plain_text();
    let remote_attachments =
        find_remote_status_attachments_by_status_id(db, &remote_status.id).await?;
    response.card = build_remote_status_card_value(&text_content, &remote_attachments);
    response.media_attachments = super::remote::remote_media_attachment_values(&remote_attachments);
    response.filtered = if viewer.is_some() {
        Some(
            super::remote::remote_status_filtered_for_viewer(
                db,
                viewer,
                &remote_status,
                &text_content,
                filter_matcher,
            )
            .await?,
        )
    } else {
        None
    };
    response.mentions = build_status_mentions(db, config, &text_content).await?;
    let (favourites_count, reblogs_count) =
        super::remote::remote_status_counts(db, counts_preload, &remote_status.id).await?;
    response.favourites_count = favourites_count;
    response.reblogs_count = reblogs_count;
    let viewer_state = super::remote::remote_status_response_viewer_state(
        db,
        viewer,
        &remote_status,
        &actor,
        None,
    )
    .await?;
    if viewer.is_some() {
        response.favourited = Some(viewer_state.favourited);
        response.reblogged = Some(viewer_state.reblogged);
        response.bookmarked = Some(viewer_state.bookmarked);
        response.muted = Some(viewer_state.muted);
    }
    response.in_reply_to_id = super::remote::resolve_remote_in_reply_to_status_id(
        db,
        config,
        remote_status.in_reply_to_uri.as_deref(),
    )
    .await?;
    response.poll = load_remote_mastodon_poll_response(db, &remote_status, viewer).await?;
    response.quote = None;
    let state = remote_quoted_status_document_state(db, viewer, &actor).await?;
    Ok(Some(quote_document_from_response(state, response)))
}
