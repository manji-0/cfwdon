use super::filters::{remote_account_status_list_options, remote_status_matches_account_filters};
use super::html::{account_statuses_html_response, remote_status_html_item};
use super::pagination::account_statuses_older_page_url;
use crate::{
    AccountStatusesQuery, AppConfig, LocalAccount, MastodonStatusResponse, RemoteActorRow,
    RemoteStatusRecord, RemoteStatusRow, Request, Response, Result,
    apply_remote_actor_social_counts, build_remote_status_response_with_timeline_preloads,
    escape_html, extract_remote_note_object, fetch_remote_activitypub_document,
    fetch_remote_actor_profile_with_document, find_follow_by_target,
    find_remote_actor_by_actor_uri, find_remote_status_attachments_by_status_ids,
    find_remote_status_ids_with_media, is_public_activitypub_visibility,
    list_public_remote_statuses_by_actor_uri, list_remote_statuses_by_actor_uri,
    load_account_filter_matcher, load_remote_actor_social_counts_from_document,
    preload_remote_mastodon_poll_responses, preload_remote_status_edit_updated_at,
    preload_remote_status_viewer_state, preload_status_counts, preload_status_quote_counts,
    remote_account_rest_id, remote_status_from_record, upsert_remote_actor, upsert_remote_status,
    visibility_from_activitypub_object,
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
            refresh_remote_status_actor(db, config, actor, true, Some(limit)).await?;
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
            load_transient_remote_actor_statuses(config, &actor, query, limit).await?;
        actor_social_counts = counts;
        statuses
    } else {
        Vec::new()
    };
    if actor_social_counts.is_none()
        && !wants_html
        && !statuses.is_empty()
        && let Ok(actor_document) = fetch_remote_activitypub_document(&actor.actor_uri).await
    {
        actor_social_counts = load_remote_actor_social_counts_from_document(&actor_document)
            .await
            .ok();
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
        if let Some(counts) = actor_social_counts {
            apply_remote_actor_social_counts(&mut status_response.account, counts);
        }
        response.push(status_response);
    }
    for mut status_response in transient_statuses {
        if let Some(counts) = actor_social_counts {
            apply_remote_actor_social_counts(&mut status_response.account, counts);
        }
        response.push(status_response);
    }
    Response::from_json(&response)
}

async fn refresh_remote_status_actor(
    db: &D1Database,
    config: &AppConfig,
    actor: RemoteActorRow,
    include_social_counts: bool,
    status_fetch_limit: Option<u32>,
) -> Result<(RemoteActorRow, Option<crate::RemoteActorSocialCounts>)> {
    let fetched = match fetch_remote_actor_profile_with_document(&actor.actor_uri).await {
        Ok(fetched) => fetched,
        Err(_) => return Ok((actor, None)),
    };
    let profile = fetched.profile;
    upsert_remote_actor(db, &profile).await?;
    if let Some(status_fetch_limit) = status_fetch_limit {
        let _ = hydrate_remote_actor_statuses_from_outbox(
            db,
            config,
            &profile,
            &fetched.document,
            status_fetch_limit,
        )
        .await;
    }
    let actor = find_remote_actor_by_actor_uri(db, &profile.actor_uri)
        .await?
        .unwrap_or(actor);
    let social_counts = if include_social_counts {
        load_remote_actor_social_counts_from_document(&fetched.document)
            .await
            .ok()
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
    let Some(outbox) = remote_actor_outbox_page(actor_document).await? else {
        return Ok(());
    };
    let mut inserted = 0usize;
    for item in activitypub_collection_items(&outbox) {
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
    Ok(())
}

async fn load_transient_remote_actor_statuses(
    config: &AppConfig,
    actor: &RemoteActorRow,
    query: &AccountStatusesQuery,
    limit: u32,
) -> Result<(
    Vec<MastodonStatusResponse>,
    Option<crate::RemoteActorSocialCounts>,
)> {
    let actor_document = fetch_remote_activitypub_document(&actor.actor_uri).await?;
    let social_counts = load_remote_actor_social_counts_from_document(&actor_document)
        .await
        .ok();
    let Some(outbox) = remote_actor_outbox_page(&actor_document).await? else {
        return Ok((Vec::new(), social_counts));
    };
    let mut response = Vec::new();
    for item in activitypub_collection_items(&outbox) {
        if response.len() >= limit as usize {
            break;
        }
        let Some(object) = extract_remote_note_object(item) else {
            continue;
        };
        if remote_status_actor_uri(object, item).as_deref() != Some(actor.actor_uri.as_str()) {
            continue;
        }
        let status = remote_status_row_from_activitypub_object(actor, object);
        if !is_public_activitypub_visibility(status.visibility.as_str()) {
            continue;
        }
        if !remote_status_matches_account_filters(&status, query, false) {
            continue;
        }
        response.push(MastodonStatusResponse::from_remote_row(
            &status, actor, config,
        ));
    }
    Ok((response, social_counts))
}

async fn remote_actor_outbox_page(
    actor_document: &serde_json::Value,
) -> Result<Option<serde_json::Value>> {
    let Some(outbox) = actor_document.get("outbox") else {
        return Ok(None);
    };
    let collection = match activitypub_reference_uri(outbox) {
        Some(uri) => fetch_remote_activitypub_document(&uri).await?,
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
        Some(uri) => fetch_remote_activitypub_document(&uri).await.map(Some),
        None if first.is_object() => Ok(Some(first.clone())),
        None => Ok(Some(collection)),
    }
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
) -> RemoteStatusRow {
    let object_uri = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    remote_status_from_record(RemoteStatusRecord {
        id: remote_account_rest_id(&object_uri),
        actor_uri: actor.actor_uri.clone(),
        object_uri,
        url: object
            .get("url")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
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
            .unwrap_or_default()
            .to_owned(),
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
    .expect("activitypub remote status record is valid")
}

fn remote_status_content_html(object: &serde_json::Value) -> String {
    object
        .get("content")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            object
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(escape_html)
        })
        .unwrap_or_default()
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
