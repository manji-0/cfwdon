use super::filters::{remote_account_status_list_options, remote_status_matches_account_filters};
use super::html::{account_statuses_html_response, remote_status_html_item};
use super::pagination::account_statuses_older_page_url;
use crate::{
    AccountStatusesQuery, AppConfig, LocalAccount, MastodonMediaAttachmentResponse,
    MastodonPollOptionResponse, MastodonPollResponse, MastodonStatusResponse, RemoteActorRow,
    RemoteCollectionFetchContext, RemoteStatusRecord, RemoteStatusRow, Request, Response, Result,
    apply_remote_actor_social_counts, build_remote_status_response_with_timeline_preloads,
    extract_remote_note_object, extract_remote_poll_draft, fetch_activitypub_document_with_context,
    fetch_remote_actor_profile_with_context, find_follow_by_target, find_remote_actor_by_actor_uri,
    find_remote_status_attachments_by_status_ids, find_remote_status_ids_with_media,
    is_public_activitypub_visibility, list_public_remote_statuses_by_actor_uri,
    list_remote_statuses_by_actor_uri, load_account_filter_matcher,
    load_remote_actor_social_counts_from_document_with_context, log_json_event,
    persist_remote_actor_social_counts, preload_remote_mastodon_poll_responses,
    preload_remote_status_edit_updated_at, preload_remote_status_viewer_state,
    preload_status_counts, preload_status_quote_counts, remote_account_rest_id,
    remote_actor_social_counts_are_fresh, remote_status_attachments_from_object,
    remote_status_from_record, sanitize_remote_http_url, sanitize_remote_plain_text,
    upsert_remote_actor, upsert_remote_status, visibility_from_activitypub_object,
};
use worker::D1Database;

struct RemoteAccountStatusPage {
    actor: RemoteActorRow,
    actor_social_counts: Option<crate::RemoteActorSocialCounts>,
    statuses: Vec<RemoteStatusRow>,
    transient_statuses: Vec<MastodonStatusResponse>,
    is_following_remote_actor: bool,
    is_pinned_page: bool,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn remote_account_statuses_response(
    req: &Request,
    config: &AppConfig,
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    actor: RemoteActorRow,
    query: &AccountStatusesQuery,
    limit: u32,
    query_limit: u32,
    wants_html: bool,
    min_id: Option<&str>,
) -> Result<Response> {
    let mut page = load_remote_account_status_page(
        db,
        config,
        viewer,
        actor,
        query,
        limit,
        query_limit,
        wants_html,
        min_id,
    )
    .await?;
    let older_page_url =
        if wants_html && !page.is_pinned_page && page.statuses.len() > limit as usize {
            page.statuses.truncate(limit as usize);
            page.statuses
                .last()
                .map(|status| account_statuses_older_page_url(req, limit, &status.id))
                .transpose()?
        } else {
            None
        };
    let status_ids = page
        .statuses
        .iter()
        .map(|status| status.id.clone())
        .collect::<Vec<_>>();
    if wants_html {
        return remote_account_statuses_html_response(
            config,
            db,
            &page.actor,
            page.statuses,
            query,
            &status_ids,
            older_page_url.as_deref(),
        )
        .await;
    }

    remote_account_statuses_json_response(db, config, viewer, page, query, &status_ids).await
}

#[allow(clippy::too_many_arguments)]
async fn load_remote_account_status_page(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    mut actor: RemoteActorRow,
    query: &AccountStatusesQuery,
    limit: u32,
    query_limit: u32,
    wants_html: bool,
    min_id: Option<&str>,
) -> Result<RemoteAccountStatusPage> {
    let is_pinned_page = query.pinned.unwrap_or(false);
    let html_fetch_limit = limit.saturating_add(1);
    let is_following_remote_actor = match viewer {
        Some(viewer) => find_follow_by_target(db, viewer.id(), &actor.actor_uri)
            .await?
            .is_some_and(|follow| follow.state == "accepted"),
        None => false,
    };
    let mut actor_social_counts = None;
    let mut statuses = if !is_following_remote_actor && !wants_html {
        Vec::new()
    } else if wants_html {
        list_public_remote_statuses_by_actor_uri(
            db,
            &actor.actor_uri,
            query.max_id.as_deref(),
            min_id,
            html_fetch_limit,
        )
        .await?
    } else {
        list_remote_statuses_by_actor_uri(
            db,
            &actor.actor_uri,
            remote_account_status_list_options(query, min_id, query_limit),
        )
        .await?
    };
    let has_visible_statuses = statuses
        .iter()
        .any(|status| remote_account_status_visible(status, is_following_remote_actor));
    if !has_visible_statuses
        && is_following_remote_actor
        && !wants_html
        && !is_pinned_page
        && !query.only_media.unwrap_or(false)
    {
        let (refreshed_actor, counts) =
            refresh_remote_status_actor(db, config, viewer, actor, true, Some(limit)).await?;
        actor = refreshed_actor;
        actor_social_counts = counts;
        statuses = list_remote_statuses_by_actor_uri(
            db,
            &actor.actor_uri,
            remote_account_status_list_options(query, min_id, query_limit),
        )
        .await?;
    }
    let transient_statuses = if !is_following_remote_actor && !wants_html {
        let (statuses, counts) =
            load_transient_remote_actor_statuses(db, config, viewer, &actor, query, min_id, limit)
                .await?;
        actor_social_counts = counts;
        statuses
    } else {
        Vec::new()
    };
    if actor_social_counts.is_none()
        && !wants_html
        && !statuses.is_empty()
        && !remote_actor_social_counts_are_fresh(actor.social_counts_updated_at.as_deref())
    {
        let fetch_context = RemoteCollectionFetchContext {
            config,
            db,
            signer: viewer,
        };
        match fetch_remote_actor_profile_with_context(&actor.actor_uri, Some(&fetch_context)).await
        {
            Ok(fetched) => {
                let counts = load_remote_actor_social_counts_from_document_with_context(
                    &fetched.document,
                    Some(&fetch_context),
                )
                .await
                .ok()
                .filter(|counts| counts.has_any());
                if let Some(counts) = counts {
                    match persist_remote_actor_social_counts(db, &actor.actor_uri, counts).await {
                        Ok(true) => actor_social_counts = Some(counts),
                        Ok(false) => {}
                        Err(error) => log_json_event(serde_json::json!({
                            "event": "remote_account_enrichment_failed",
                            "actor_uri": actor.actor_uri,
                            "stage": "social_counts",
                            "error": error.to_string(),
                        })),
                    }
                }
            }
            Err(error) => log_json_event(serde_json::json!({
                "event": "remote_account_enrichment_failed",
                "actor_uri": actor.actor_uri,
                "stage": "actor_document",
                "error": error.to_string(),
            })),
        }
    }

    Ok(RemoteAccountStatusPage {
        actor,
        actor_social_counts,
        statuses,
        transient_statuses,
        is_following_remote_actor,
        is_pinned_page,
    })
}

async fn remote_account_statuses_html_response(
    config: &AppConfig,
    db: &D1Database,
    actor: &RemoteActorRow,
    statuses: Vec<RemoteStatusRow>,
    query: &AccountStatusesQuery,
    status_ids: &[String],
    older_page_url: Option<&str>,
) -> Result<Response> {
    let mut remote_attachments_by_status_id =
        find_remote_status_attachments_by_status_ids(db, status_ids).await?;
    let mut html_statuses = Vec::new();

    for status in statuses {
        if !is_public_activitypub_visibility(status.visibility.as_str()) {
            continue;
        }
        let media = remote_attachments_by_status_id
            .remove(&status.id)
            .unwrap_or_default();
        if !remote_status_matches_account_filters(&status, query, !media.is_empty()) {
            continue;
        }

        html_statuses.push(remote_status_html_item(actor, &status, &media));
    }

    let profile_url = actor
        .profile_url
        .clone()
        .unwrap_or_else(|| actor.actor_uri.clone());
    account_statuses_html_response(
        config,
        &actor.display_name,
        &format!("{}@{}", actor.username, actor.domain),
        &profile_url,
        &html_statuses,
        older_page_url,
    )
}

async fn remote_account_statuses_json_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    page: RemoteAccountStatusPage,
    query: &AccountStatusesQuery,
    status_ids: &[String],
) -> Result<Response> {
    let RemoteAccountStatusPage {
        actor,
        actor_social_counts,
        statuses,
        transient_statuses,
        is_following_remote_actor,
        ..
    } = page;
    let remote_status_refs = statuses
        .iter()
        .map(|status| (status, &actor))
        .collect::<Vec<_>>();
    let quote_uris = statuses
        .iter()
        .map(|status| status.object_uri.clone())
        .collect::<Vec<_>>();
    let (
        counts_preload,
        quote_counts_preload,
        viewer_state_preload,
        poll_preload,
        edit_updated_at_preload,
        mut remote_attachments_by_status_id,
        remote_status_ids_with_media,
    ) = futures_util::try_join!(
        preload_status_counts(db, &[], status_ids),
        preload_status_quote_counts(db, &quote_uris),
        async {
            match viewer {
                Some(viewer) => {
                    preload_remote_status_viewer_state(db, viewer.id(), &remote_status_refs).await
                }
                None => Ok(Default::default()),
            }
        },
        preload_remote_mastodon_poll_responses(db, status_ids, viewer),
        preload_remote_status_edit_updated_at(db, status_ids),
        find_remote_status_attachments_by_status_ids(db, status_ids),
        find_remote_status_ids_with_media(db, status_ids),
    )?;
    let filter_matcher = match viewer {
        Some(viewer) => Some(load_account_filter_matcher(db, viewer.id()).await?),
        None => None,
    };
    let mut response = Vec::new();
    for status in statuses {
        if !remote_account_status_visible(&status, is_following_remote_actor) {
            continue;
        }
        if !remote_status_matches_account_filters(
            &status,
            query,
            remote_status_ids_with_media.contains(&status.id),
        ) {
            continue;
        }

        let mut status_response = build_remote_status_response_with_timeline_preloads(
            db,
            config,
            viewer,
            &status,
            &actor,
            filter_matcher.as_ref(),
            Some(&counts_preload),
            Some(&quote_counts_preload),
            Some(&viewer_state_preload),
            Some(&poll_preload),
            Some(&edit_updated_at_preload),
            remote_attachments_by_status_id
                .remove(&status.id)
                .unwrap_or_default(),
            None,
        )
        .await?;
        if let Some(counts) = actor_social_counts
            && counts.has_any()
        {
            apply_remote_actor_social_counts(&mut status_response.account, counts);
        }
        response.push(status_response);
    }
    for mut status_response in transient_statuses {
        if let Some(counts) = actor_social_counts
            && counts.has_any()
        {
            apply_remote_actor_social_counts(&mut status_response.account, counts);
        }
        response.push(status_response);
    }
    Response::from_json(&response)
}

async fn refresh_remote_status_actor(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    actor: RemoteActorRow,
    include_social_counts: bool,
    status_fetch_limit: Option<u32>,
) -> Result<(RemoteActorRow, Option<crate::RemoteActorSocialCounts>)> {
    let fetch_context = RemoteCollectionFetchContext {
        config,
        db,
        signer: viewer,
    };
    let fetched =
        match fetch_remote_actor_profile_with_context(&actor.actor_uri, Some(&fetch_context)).await
        {
            Ok(fetched) => fetched,
            Err(error) => {
                log_json_event(serde_json::json!({
                    "event": "remote_actor_refresh_failed",
                    "actor_uri": actor.actor_uri,
                    "error": error.to_string(),
                }));
                return Ok((actor, None));
            }
        };
    let profile = fetched.profile;
    upsert_remote_actor(db, &profile).await?;
    if let Some(status_fetch_limit) = status_fetch_limit
        && let Err(error) = hydrate_remote_actor_statuses_from_outbox(
            db,
            config,
            &profile,
            &fetched.document,
            status_fetch_limit,
        )
        .await
    {
        log_json_event(serde_json::json!({
            "event": "remote_outbox_hydrate_failed",
            "actor_uri": profile.actor_uri,
            "error": error.to_string(),
        }));
    }
    let actor = find_remote_actor_by_actor_uri(db, &profile.actor_uri)
        .await?
        .unwrap_or(actor);
    let social_counts = if include_social_counts
        && !remote_actor_social_counts_are_fresh(actor.social_counts_updated_at.as_deref())
    {
        let counts = load_remote_actor_social_counts_from_document_with_context(
            &fetched.document,
            Some(&fetch_context),
        )
        .await
        .ok()
        .filter(|counts| counts.has_any());
        if let Some(counts) = counts {
            match persist_remote_actor_social_counts(db, &profile.actor_uri, counts).await {
                Ok(true) => Some(counts),
                Ok(false) => None,
                Err(error) => {
                    log_json_event(serde_json::json!({
                        "event": "remote_account_enrichment_failed",
                        "actor_uri": profile.actor_uri,
                        "stage": "social_counts",
                        "error": error.to_string(),
                    }));
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };
    Ok((actor, social_counts))
}

async fn hydrate_remote_actor_statuses_from_outbox(
    db: &D1Database,
    config: &AppConfig,
    actor: &crate::RemoteActorProfile,
    actor_document: &serde_json::Value,
    limit: u32,
) -> Result<()> {
    let limit = limit.clamp(1, 20) as usize;
    let Some(mut page) = remote_actor_outbox_page(actor_document, None).await? else {
        return Ok(());
    };
    let mut inserted = 0usize;
    let mut pages_fetched = 0usize;
    while inserted < limit && pages_fetched < TRANSIENT_OUTBOX_MAX_PAGES {
        pages_fetched += 1;
        for item in activitypub_collection_items(&page) {
            if inserted >= limit {
                break;
            }
            let Some(object) = extract_remote_note_object(item) else {
                continue;
            };
            if remote_status_actor_uri(object, item).as_deref() != Some(actor.actor_uri.as_str()) {
                continue;
            }
            upsert_remote_status(db, config, actor, object).await?;
            inserted += 1;
        }
        if inserted >= limit {
            break;
        }
        let Some(next) = activitypub_collection_next_uri(&page) else {
            break;
        };
        match fetch_activitypub_document_with_context(&next, None).await {
            Ok(next_page) => page = next_page,
            Err(error) => {
                log_json_event(serde_json::json!({
                    "event": "remote_outbox_page_fetch_failed",
                    "actor_uri": actor.actor_uri,
                    "url": next,
                    "error": error.to_string(),
                }));
                break;
            }
        }
    }
    Ok(())
}

async fn load_transient_remote_actor_statuses(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    actor: &RemoteActorRow,
    query: &AccountStatusesQuery,
    min_id: Option<&str>,
    limit: u32,
) -> Result<(
    Vec<MastodonStatusResponse>,
    Option<crate::RemoteActorSocialCounts>,
)> {
    let fetch_context = RemoteCollectionFetchContext {
        config,
        db,
        signer: viewer,
    };
    let fetched =
        fetch_remote_actor_profile_with_context(&actor.actor_uri, Some(&fetch_context)).await?;
    let actor_document = fetched.document;
    let social_counts =
        if remote_actor_social_counts_are_fresh(actor.social_counts_updated_at.as_deref()) {
            None
        } else {
            match load_remote_actor_social_counts_from_document_with_context(
                &actor_document,
                Some(&fetch_context),
            )
            .await
            {
                Ok(counts) if counts.has_any() => {
                    match persist_remote_actor_social_counts(db, &actor.actor_uri, counts).await {
                        Ok(true) => Some(counts),
                        Ok(false) => None,
                        Err(error) => {
                            log_json_event(serde_json::json!({
                                "event": "remote_account_enrichment_failed",
                                "actor_uri": actor.actor_uri,
                                "stage": "social_counts",
                                "error": error.to_string(),
                            }));
                            None
                        }
                    }
                }
                Ok(_) => None,
                Err(error) => {
                    log_json_event(serde_json::json!({
                        "event": "remote_account_enrichment_failed",
                        "actor_uri": actor.actor_uri,
                        "stage": "social_counts",
                        "error": error.to_string(),
                    }));
                    None
                }
            }
        };
    if query.pinned.unwrap_or(false) {
        return Ok((Vec::new(), social_counts));
    }
    let Some(mut page) = remote_actor_outbox_page(&actor_document, Some(&fetch_context)).await?
    else {
        return Ok((Vec::new(), social_counts));
    };
    let limit = limit.clamp(1, 20) as usize;
    let max_id = query.max_id.as_deref();
    let mut response = Vec::new();
    let mut pages_fetched = 0usize;
    let mut passed_max_id = max_id.is_none();
    while response.len() < limit && pages_fetched < TRANSIENT_OUTBOX_MAX_PAGES {
        pages_fetched += 1;
        for item in activitypub_collection_items(&page) {
            if response.len() >= limit {
                break;
            }
            let Some(object) = extract_remote_note_object(item) else {
                continue;
            };
            if remote_status_actor_uri(object, item).as_deref() != Some(actor.actor_uri.as_str()) {
                continue;
            }
            let Ok(status) = remote_status_row_from_activitypub_object(actor, object) else {
                continue;
            };
            if let Some(max_id) = max_id {
                if status.id == max_id {
                    passed_max_id = true;
                    continue;
                }
                if !passed_max_id {
                    continue;
                }
            }
            if let Some(min_id) = min_id
                && status.id == min_id
            {
                // Exclusive lower bound reached in reverse-chronological order.
                return Ok((response, social_counts));
            }
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
                continue;
            }
            let attachments = remote_status_attachments_from_object(&status.id, object);
            if !remote_status_matches_account_filters(&status, query, !attachments.is_empty()) {
                continue;
            }
            response.push(transient_mastodon_status_response(
                config,
                actor,
                &status,
                object,
                &attachments,
            ));
        }
        if response.len() >= limit {
            break;
        }
        let Some(next) = activitypub_collection_next_uri(&page) else {
            break;
        };
        match fetch_activitypub_document_with_context(&next, Some(&fetch_context)).await {
            Ok(next_page) => page = next_page,
            Err(error) => {
                log_json_event(serde_json::json!({
                    "event": "remote_outbox_page_fetch_failed",
                    "actor_uri": actor.actor_uri,
                    "url": next,
                    "error": error.to_string(),
                }));
                break;
            }
        }
    }
    Ok((response, social_counts))
}

fn transient_mastodon_status_response(
    config: &AppConfig,
    actor: &RemoteActorRow,
    status: &RemoteStatusRow,
    object: &serde_json::Value,
    attachments: &[crate::RemoteStatusAttachmentRow],
) -> MastodonStatusResponse {
    let mut response = MastodonStatusResponse::from_remote_row(status, actor, config);
    response.media_attachments = attachments
        .iter()
        .map(|attachment| {
            serde_json::to_value(MastodonMediaAttachmentResponse::from_remote_row(attachment))
                .unwrap_or(serde_json::Value::Null)
        })
        .collect();
    if let Some(poll) = extract_remote_poll_draft(object) {
        response.poll = serde_json::to_value(MastodonPollResponse {
            id: status.id.clone(),
            expires_at: poll.expires_at.unwrap_or_default(),
            expired: poll.expired,
            multiple: poll.multiple,
            votes_count: poll.votes_count,
            voters_count: poll.voters_count,
            voted: None,
            own_votes: None,
            options: poll
                .options
                .into_iter()
                .map(|option| MastodonPollOptionResponse {
                    title: option.title,
                    votes_count: Some(option.votes_count),
                })
                .collect(),
            emojis: Vec::new(),
        })
        .ok();
    }
    response
}

const TRANSIENT_OUTBOX_MAX_PAGES: usize = 5;

async fn remote_actor_outbox_page(
    actor_document: &serde_json::Value,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
) -> Result<Option<serde_json::Value>> {
    let Some(outbox) = actor_document.get("outbox") else {
        return Ok(None);
    };
    let collection = match activitypub_reference_uri(outbox) {
        Some(uri) => fetch_activitypub_document_with_context(&uri, fetch_context).await?,
        None if outbox.is_object() => outbox.clone(),
        None => return Ok(None),
    };
    if !activitypub_collection_items(&collection).is_empty() {
        return Ok(Some(collection));
    }
    let Some(first) = collection.get("first") else {
        return Ok(Some(collection));
    };
    match activitypub_reference_uri(first) {
        Some(uri) => fetch_activitypub_document_with_context(&uri, fetch_context)
            .await
            .map(Some),
        None if first.is_object() => Ok(Some(first.clone())),
        None => Ok(Some(collection)),
    }
}

fn activitypub_collection_next_uri(collection: &serde_json::Value) -> Option<String> {
    collection.get("next").and_then(activitypub_reference_uri)
}

fn activitypub_collection_items(collection: &serde_json::Value) -> Vec<&serde_json::Value> {
    collection
        .get("orderedItems")
        .or_else(|| collection.get("items"))
        .and_then(serde_json::Value::as_array)
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

fn activitypub_reference_uri(value: &serde_json::Value) -> Option<String> {
    if let Some(uri) = value.as_str().map(str::trim).filter(|uri| !uri.is_empty()) {
        return Some(uri.to_owned());
    }
    value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|uri| !uri.is_empty())
        .map(ToOwned::to_owned)
}

fn remote_status_actor_uri(
    object: &serde_json::Value,
    activity: &serde_json::Value,
) -> Option<String> {
    object
        .get("attributedTo")
        .and_then(serde_json::Value::as_str)
        .or_else(|| activity.get("actor").and_then(serde_json::Value::as_str))
        .map(ToOwned::to_owned)
}

fn remote_status_row_from_activitypub_object(
    actor: &RemoteActorRow,
    object: &serde_json::Value,
) -> Result<RemoteStatusRow> {
    let object_uri = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    remote_status_from_record(RemoteStatusRecord {
        id: remote_account_rest_id(&object_uri),
        actor_uri: actor.actor_uri.clone(),
        object_uri,
        url: sanitize_remote_http_url(object.get("url").and_then(serde_json::Value::as_str)),
        in_reply_to_uri: object
            .get("inReplyTo")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        boost_of_uri: None,
        quote_of_uri: None,
        content_html: remote_status_content_html(object),
        spoiler_text: object
            .get("summary")
            .and_then(serde_json::Value::as_str)
            .map(sanitize_remote_plain_text)
            .unwrap_or_default(),
        visibility: visibility_from_activitypub_object(object),
        sensitive: i32::from(
            object
                .get("sensitive")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        ),
        language: object
            .get("contentMap")
            .and_then(serde_json::Value::as_object)
            .and_then(|map| map.keys().next().cloned()),
        quote_state: "accepted".to_owned(),
        published_at: remote_status_published_at(object),
    })
}

fn remote_status_content_html(object: &serde_json::Value) -> String {
    crate::remote_status_content_html(object)
}

fn remote_status_published_at(object: &serde_json::Value) -> String {
    object
        .get("published")
        .and_then(serde_json::Value::as_str)
        .or_else(|| object.get("updated").and_then(serde_json::Value::as_str))
        .unwrap_or_default()
        .to_owned()
}

fn remote_account_status_visible(status: &RemoteStatusRow, is_following_actor: bool) -> bool {
    is_public_activitypub_visibility(status.visibility.as_str())
        || (is_following_actor && status.visibility == cfwdon_domain::Visibility::FollowersOnly)
}
