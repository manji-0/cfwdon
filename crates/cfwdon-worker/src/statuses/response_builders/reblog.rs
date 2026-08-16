use super::super::reblog_response::{
    local_reblog_wrapper_response_from_embedded, remote_reblog_wrapper_response_from_embedded,
};
use super::super::{
    AccountFilterMatcher, AppConfig, BoostTarget, BoostTargetPreload, LocalAccount,
    LocalStatusViewerStatePreload, MastodonPollResponsePreload, MastodonStatusResponse,
    MentionAccountsPreload, RemoteActorRow, RemoteMastodonPollResponsePreload,
    RemoteStatusAttachmentRow, RemoteStatusEditUpdatedAtPreload,
    RemoteStatusFederatedEmojisPreload, RemoteStatusRow, RemoteStatusViewerStatePreload,
    StatusApplicationPreload, StatusCountsPreload, StatusQuoteCountsPreload, StatusRow,
    find_remote_actor_by_actor_uri, resolve_boost_target, resolve_local_status_response_subject,
};
use std::collections::HashMap;
use worker::Result;

use crate::D1Database;

pub(super) async fn build_remote_reblog_wrapper_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    wrapper_status: &RemoteStatusRow,
    wrapper_actor: &RemoteActorRow,
    boost_of_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    viewer_state_preload: Option<&RemoteStatusViewerStatePreload>,
    poll_preload: Option<&RemoteMastodonPollResponsePreload>,
    edit_updated_at_preload: Option<&RemoteStatusEditUpdatedAtPreload>,
    federated_emojis_preload: Option<&RemoteStatusFederatedEmojisPreload>,
    mention_preload: Option<&MentionAccountsPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
    remote_in_reply_to_preload: Option<&HashMap<String, Option<String>>>,
    remote_actors_preload: Option<&HashMap<String, RemoteActorRow>>,
    remote_attachments_preload: Option<&HashMap<String, Vec<RemoteStatusAttachmentRow>>>,
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    let embedded = build_reblog_embedded_response(
        db,
        config,
        None,
        viewer,
        boost_of_uri,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        None,
        None,
        None,
        boost_target_preload,
        viewer_state_preload,
        poll_preload,
        edit_updated_at_preload,
        federated_emojis_preload,
        mention_preload,
        remote_in_reply_to_preload,
        remote_actors_preload,
        remote_attachments_preload,
        include_quote,
    )
    .await?;

    Ok(remote_reblog_wrapper_response_from_embedded(
        embedded,
        wrapper_status,
        wrapper_actor,
        config,
    ))
}

async fn build_reblog_embedded_response(
    db: &D1Database,
    config: &AppConfig,
    resolved_config: Option<&AppConfig>,
    viewer: Option<&LocalAccount>,
    boost_of_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
    remote_viewer_state_preload: Option<&RemoteStatusViewerStatePreload>,
    remote_poll_preload: Option<&RemoteMastodonPollResponsePreload>,
    remote_edit_updated_at_preload: Option<&RemoteStatusEditUpdatedAtPreload>,
    federated_emojis_preload: Option<&RemoteStatusFederatedEmojisPreload>,
    mention_preload: Option<&MentionAccountsPreload>,
    remote_in_reply_to_preload: Option<&HashMap<String, Option<String>>>,
    remote_actors_preload: Option<&HashMap<String, RemoteActorRow>>,
    remote_attachments_preload: Option<&HashMap<String, Vec<RemoteStatusAttachmentRow>>>,
    include_quote: bool,
) -> Result<Option<MastodonStatusResponse>> {
    let target = match boost_target_preload.and_then(|targets| targets.target(boost_of_uri)) {
        Some(target) => target.cloned(),
        None => resolve_boost_target(db, config, boost_of_uri).await?,
    };

    match target {
        Some(BoostTarget::Local(local_status)) => {
            build_local_reblog_embedded_response(
                db,
                config,
                resolved_config,
                viewer,
                local_status,
                filter_matcher,
                counts_preload,
                quote_counts_preload,
                poll_preload,
                viewer_state_preload,
                application_preload,
                boost_target_preload,
                include_quote,
            )
            .await
        }
        Some(BoostTarget::Remote(remote_status)) => {
            build_remote_reblog_embedded_response(
                db,
                config,
                viewer,
                remote_status,
                filter_matcher,
                counts_preload,
                quote_counts_preload,
                remote_viewer_state_preload,
                remote_poll_preload,
                remote_edit_updated_at_preload,
                federated_emojis_preload,
                mention_preload,
                boost_target_preload,
                remote_in_reply_to_preload,
                remote_actors_preload,
                remote_attachments_preload,
                include_quote,
            )
            .await
        }
        None => Ok(None),
    }
}

async fn build_local_reblog_embedded_response(
    db: &D1Database,
    config: &AppConfig,
    resolved_config: Option<&AppConfig>,
    viewer: Option<&LocalAccount>,
    local_status: StatusRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
    include_quote: bool,
) -> Result<Option<MastodonStatusResponse>> {
    let Some(subject) = resolve_local_status_response_subject(db, viewer, local_status).await?
    else {
        return Ok(None);
    };
    let super::super::ResolvedLocalStatusResponseSubject::Loaded(subject) = subject else {
        return Ok(None);
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
    Ok(Some(
        Box::pin(super::local::build_local_status_response_inner(
            db,
            config,
            resolved_config,
            viewer,
            &local_status,
            &local_account,
            in_reply_to_account_id,
            media,
            filter_matcher,
            counts_preload,
            quote_counts_preload,
            poll_preload,
            viewer_state_preload,
            application_preload,
            None,
            boost_target_preload,
            include_quote,
        ))
        .await?,
    ))
}

async fn build_remote_reblog_embedded_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    remote_status: RemoteStatusRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    viewer_state_preload: Option<&RemoteStatusViewerStatePreload>,
    poll_preload: Option<&RemoteMastodonPollResponsePreload>,
    edit_updated_at_preload: Option<&RemoteStatusEditUpdatedAtPreload>,
    federated_emojis_preload: Option<&RemoteStatusFederatedEmojisPreload>,
    mention_preload: Option<&MentionAccountsPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
    remote_in_reply_to_preload: Option<&HashMap<String, Option<String>>>,
    remote_actors_preload: Option<&HashMap<String, RemoteActorRow>>,
    remote_attachments_preload: Option<&HashMap<String, Vec<RemoteStatusAttachmentRow>>>,
    include_quote: bool,
) -> Result<Option<MastodonStatusResponse>> {
    if !matches!(remote_status.visibility.as_str(), "public" | "unlisted") {
        return Ok(None);
    }

    let preloaded_actor =
        remote_actors_preload.and_then(|actors| actors.get(&remote_status.actor_uri));
    let looked_up_actor;
    let actor = if let Some(actor) = preloaded_actor {
        actor
    } else {
        looked_up_actor = match find_remote_actor_by_actor_uri(db, &remote_status.actor_uri).await?
        {
            Some(actor) => actor,
            None => return Ok(None),
        };
        &looked_up_actor
    };

    let remote_attachments = remote_attachments_preload
        .and_then(|attachments| attachments.get(&remote_status.id).cloned());

    Ok(Some(
        Box::pin(super::remote::build_remote_status_response_inner(
            db,
            config,
            viewer,
            &remote_status,
            actor,
            filter_matcher,
            counts_preload,
            quote_counts_preload,
            viewer_state_preload,
            poll_preload,
            edit_updated_at_preload,
            federated_emojis_preload,
            remote_attachments,
            mention_preload,
            boost_target_preload,
            remote_in_reply_to_preload,
            remote_actors_preload,
            remote_attachments_preload,
            include_quote,
        ))
        .await?,
    ))
}

pub(super) async fn build_local_reblog_wrapper_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    wrapper_status: &StatusRow,
    wrapper_account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    boost_of_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    // The only caller resolves the emoji registry before delegating here, so the
    // embedded status can reuse it instead of re-reading `custom_emojis`.
    let embedded = build_reblog_embedded_response(
        db,
        config,
        Some(config),
        viewer,
        boost_of_uri,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        poll_preload,
        viewer_state_preload,
        application_preload,
        boost_target_preload,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        include_quote,
    )
    .await?;

    Ok(local_reblog_wrapper_response_from_embedded(
        embedded,
        wrapper_status,
        wrapper_account,
        in_reply_to_account_id,
        config,
    ))
}
