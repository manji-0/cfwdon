//! Status API response builders.
//!
//! Local and remote status response entry points in this module are intentional
//! graph bridges: timelines, quotes, and detail routes converge here so shared
//! preload/viewer/quote embedding stays consistent. Prefer extracting cohesive
//! helpers (mentions, quote documents) into sibling modules rather than adding
//! more route-specific forks.

mod local;
mod remote;

#[allow(unused_imports)]
pub(crate) use local::{
    build_loaded_local_status_response, build_local_status_response,
    build_local_status_response_with_filter_matcher, build_local_status_response_with_preloads,
    build_local_status_response_with_quote_count_preloads,
    build_local_status_response_with_timeline_preloads,
};
#[allow(unused_imports)]
pub(crate) use remote::{
    build_remote_status_response, build_remote_status_response_with_filter_matcher,
    build_remote_status_response_with_preloads,
    build_remote_status_response_with_timeline_preloads,
};

use self::local::{
    build_local_status_response_inner, local_status_counts, local_status_filtered_for_viewer,
    local_status_response_viewer_state, local_viewer_interaction_fields,
};
use self::remote::{
    build_remote_status_response_inner, federated_emojis_for_remote_status,
    remote_media_attachment_values, remote_status_counts, remote_status_filtered_for_viewer,
    remote_status_response_viewer_state, resolve_remote_in_reply_to_status_id,
};

use super::reblog_response::{
    local_reblog_wrapper_response_from_embedded, remote_reblog_wrapper_response_from_embedded,
};
use super::{
    AccountFilterMatcher, AppConfig, BoostTarget, BoostTargetPreload, LocalAccount,
    LocalStatusViewerStatePreload, MastodonPollResponsePreload, MastodonStatusResponse,
    MentionAccountsPreload, RemoteActorRow, RemoteMastodonPollResponsePreload,
    RemoteStatusAttachmentRow, RemoteStatusEditUpdatedAtPreload,
    RemoteStatusFederatedEmojisPreload, RemoteStatusRow, RemoteStatusViewerStatePreload,
    StatusApplicationPreload, StatusCountsPreload, StatusQuoteCountsPreload, StatusRow,
    build_remote_status_card_value, build_status_card_value, build_status_mentions,
    find_local_status_by_object_uri, find_remote_actor_by_actor_uri,
    find_remote_status_attachments_by_status_id, find_remote_status_by_url_or_object_uri,
    load_mastodon_poll_response, load_remote_mastodon_poll_response, load_status_filtered,
    local_quoted_status_document_state, pending_quote_document, quote_document_for_local_state,
    quote_document_from_response, remote_quote_visibility_is_embeddable,
    remote_quoted_status_document_state, resolve_boost_target,
    resolve_local_status_response_subject, unauthorized_quote_document,
};
use std::collections::HashMap;
use worker::Result;

use crate::D1Database;

async fn build_quoted_status_value(
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
    let super::ResolvedLocalStatusResponseSubject::Loaded(subject) = subject else {
        return Ok(Some(unauthorized_quote_document()));
    };
    let super::LoadedLocalStatusResponseSubject {
        status: local_status,
        account: local_account,
        preload:
            super::LocalStatusResponsePreload {
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
        Some(local_status_filtered_for_viewer(db, viewer, &local_status, filter_matcher).await?)
    } else {
        None
    };
    response.mentions = build_status_mentions(db, config, &local_status.text).await?;
    let (favourites_count, reblogs_count) =
        local_status_counts(db, counts_preload, &local_status.id).await?;
    response.favourites_count = favourites_count;
    response.reblogs_count = reblogs_count;
    let viewer_state = local_status_response_viewer_state(db, viewer, &local_status, None).await?;
    let viewer_fields = local_viewer_interaction_fields(viewer, &local_status, viewer_state);
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
    let federated_emojis = federated_emojis_for_remote_status(db, &remote_status.id, None).await?;
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
    response.media_attachments = remote_media_attachment_values(&remote_attachments);
    response.filtered = if viewer.is_some() {
        Some(
            remote_status_filtered_for_viewer(
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
        remote_status_counts(db, counts_preload, &remote_status.id).await?;
    response.favourites_count = favourites_count;
    response.reblogs_count = reblogs_count;
    let viewer_state =
        remote_status_response_viewer_state(db, viewer, &remote_status, &actor, None).await?;
    if viewer.is_some() {
        response.favourited = Some(viewer_state.favourited);
        response.reblogged = Some(viewer_state.reblogged);
        response.bookmarked = Some(viewer_state.bookmarked);
        response.muted = Some(viewer_state.muted);
    }
    response.in_reply_to_id =
        resolve_remote_in_reply_to_status_id(db, config, remote_status.in_reply_to_uri.as_deref())
            .await?;
    response.poll = load_remote_mastodon_poll_response(db, &remote_status, viewer).await?;
    response.quote = None;
    let state = remote_quoted_status_document_state(db, viewer, &actor).await?;
    Ok(Some(quote_document_from_response(state, response)))
}

async fn filtered_status_for_viewer(
    db: &D1Database,
    filter_matcher: Option<&AccountFilterMatcher>,
    account_id: &str,
    status_id: &str,
    text: &str,
    spoiler_text: &str,
) -> Result<Vec<serde_json::Value>> {
    if let Some(filter_matcher) = filter_matcher {
        return Ok(filter_matcher.filtered_status(status_id, text, spoiler_text));
    }

    load_status_filtered(db, account_id, status_id, text, spoiler_text).await
}

async fn build_remote_reblog_wrapper_response(
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
    let super::ResolvedLocalStatusResponseSubject::Loaded(subject) = subject else {
        return Ok(None);
    };
    let super::LoadedLocalStatusResponseSubject {
        status: local_status,
        account: local_account,
        preload:
            super::LocalStatusResponsePreload {
                media,
                in_reply_to_account_id,
            },
    } = subject;
    Ok(Some(
        Box::pin(build_local_status_response_inner(
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
        Box::pin(build_remote_status_response_inner(
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

async fn build_local_reblog_wrapper_response(
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

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_domain::LocalAccountRecord;

    #[test]
    fn remote_media_attachment_values_allows_empty_attachments() {
        assert!(remote_media_attachment_values(&[]).is_empty());
    }

    #[test]
    fn remote_reblog_wrapper_response_overlays_wrapper_fields_and_clears_embedded_body() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let wrapper_actor = remote_actor_row_fixture();
        let wrapper_status =
            remote_status_row_fixture("wrapper-status", "https://remote.example/announce/1");
        let embedded_status =
            remote_status_row_fixture("embedded-status", "https://remote.example/statuses/1");
        let mut embedded = MastodonStatusResponse::from_remote_row(
            &embedded_status,
            &wrapper_actor,
            &config,
            None,
        );
        embedded.media_attachments = vec![serde_json::json!({"id": "media-1"})];
        embedded.quote = Some(serde_json::json!({"state": "accepted"}));

        let response = remote_reblog_wrapper_response_from_embedded(
            Some(embedded),
            &wrapper_status,
            &wrapper_actor,
            &config,
        );

        assert_eq!(response.id, "wrapper-status");
        assert_eq!(response.uri, "https://remote.example/announce/1");
        assert!(response.reblog.is_some());
        assert!(response.content.is_empty());
        assert!(response.media_attachments.is_empty());
        assert!(response.quote.is_none());
    }

    #[test]
    fn local_reblog_wrapper_response_overlays_wrapper_fields_and_clears_embedded_body() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let wrapper_account = local_account_fixture();
        let wrapper_status = status_row_fixture(
            "wrapper-status",
            Some("https://social.example/users/alice/statuses/wrapper"),
        );
        let embedded_status = status_row_fixture(
            "embedded-status",
            Some("https://social.example/users/alice/statuses/embedded"),
        );
        let mut embedded = MastodonStatusResponse::from_row(
            &embedded_status,
            &wrapper_account,
            &config,
            None,
            Vec::new(),
        );
        embedded.media_attachments = vec![serde_json::json!({"id": "media-1"})];
        embedded.quote = Some(serde_json::json!({"state": "accepted"}));

        let response = local_reblog_wrapper_response_from_embedded(
            Some(embedded),
            &wrapper_status,
            &wrapper_account,
            Some("reply-account".to_owned()),
            &config,
        );

        assert_eq!(response.id, "wrapper-status");
        assert_eq!(
            response.uri,
            "https://social.example/users/alice/statuses/wrapper"
        );
        assert_eq!(
            response.in_reply_to_account_id.as_deref(),
            Some("reply-account")
        );
        assert!(response.reblog.is_some());
        assert!(response.content.is_empty());
        assert!(response.media_attachments.is_empty());
        assert!(response.quote.is_none());
    }

    fn remote_status_row_fixture(id: &str, object_uri: &str) -> RemoteStatusRow {
        RemoteStatusRow {
            id: id.to_owned(),
            actor_uri: "https://remote.example/users/alice".to_owned(),
            object_uri: object_uri.to_owned(),
            url: None,
            in_reply_to_uri: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>Hello</p>".to_owned(),
            text_content: "Hello".to_owned(),
            spoiler_text: String::new(),
            visibility: cfwdon_domain::Visibility::Public,
            sensitive: false,
            language: Some("en".to_owned()),
            quote_state: cfwdon_domain::QuoteState::Accepted,
            published_at: "2026-05-10T01:02:03Z".to_owned(),
            edited_at: None,
            card_json: None,
            federated_emojis_json: "[]".to_owned(),
            in_reply_to_id: None,
        }
    }

    fn remote_actor_row_fixture() -> RemoteActorRow {
        RemoteActorRow {
            actor_uri: "https://remote.example/users/alice".to_owned(),
            username: "alice".to_owned(),
            domain: "remote.example".to_owned(),
            created_at: "2026-05-01T00:00:00Z".to_owned(),
            locked: false,
            bot: false,
            discoverable: true,
            indexable: true,
            display_name: "Alice".to_owned(),
            summary_html: String::new(),
            profile_url: Some("https://remote.example/@alice".to_owned()),
            avatar_url: None,
            header_url: None,
            followers_count: 0,
            following_count: 0,
            statuses_count: 0,
            social_counts_updated_at: None,
        }
    }

    fn status_row_fixture(id: &str, ap_id: Option<&str>) -> StatusRow {
        StatusRow {
            id: id.to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: ap_id.map(str::to_owned),
            in_reply_to_id: None,
            in_reply_to_account_id: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>Hello</p>".to_owned(),
            text: "Hello".to_owned(),
            spoiler_text: String::new(),
            visibility: cfwdon_domain::Visibility::Public,
            sensitive: false,
            language: Some("en".to_owned()),
            quote_approval_policy: None,
            quote_state: cfwdon_domain::QuoteState::Accepted,
            application_id: None,
            card_json: None,
            created_at: "2026-05-10T01:02:03Z".to_owned(),
            updated_at: None,
        }
    }

    fn local_account_fixture() -> LocalAccount {
        LocalAccount::from_record(LocalAccountRecord::test_fixture("acct-1", "alice"))
    }
}
