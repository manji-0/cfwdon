use crate::auth::{extract_authenticated_user, find_account_by_id, resolve_local_account};
use crate::content_helpers::{extract_hashtags_from_html, extract_hashtags_from_text};
use crate::find_media_attachments_by_status_id;
use crate::instance_identity::actor_url;
use crate::runtime_config::load_config;
use crate::{
    HomeTimelineQuery, PublicTimelineQuery, TagTimelineQuery, build_local_status_response,
    build_remote_status_response, build_timeline_link_header, include_local_source,
    include_remote_source, list_local_home_timeline_statuses, list_local_public_statuses_by_tag,
    list_local_public_timeline_statuses, list_remote_home_timeline_statuses,
    list_remote_public_statuses_by_tag, list_remote_public_timeline_statuses,
    load_in_reply_to_account_id, matches_tag_timeline_filters, normalize_hashtag,
    remote_status_has_media, resolve_timeline_cursor, timeline_fetch_limit, timeline_limit,
};
use crate::{is_local_status_thread_muted_by, is_muted_actor};
use worker::{Error, Request, Response, Result, RouteContext};

pub(crate) async fn home_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query: HomeTimelineQuery = req.query().unwrap_or_default();
    let limit = timeline_limit(&query.pagination);
    let query_limit = timeline_fetch_limit(limit);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = resolve_local_account(&db, &user).await?;
    let cursor = resolve_timeline_cursor(&db, &query.pagination).await?;
    let mut entries = Vec::new();

    for status in list_local_home_timeline_statuses(&db, &viewer.id, &cursor, query_limit).await? {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            continue;
        };
        if is_muted_actor(&db, &viewer.id, &actor_url(&config, &account.username)).await? {
            continue;
        }
        if is_local_status_thread_muted_by(&db, &viewer.id, &status).await? {
            continue;
        }
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        entries.push((
            status.created_at.clone(),
            status.id.clone(),
            build_local_status_response(
                &db,
                &config,
                Some(&viewer),
                &status,
                &account,
                load_in_reply_to_account_id(&db, &status).await?,
                media,
            )
            .await?,
        ));
    }

    for (status, actor) in
        list_remote_home_timeline_statuses(&db, &viewer.id, &cursor, query_limit).await?
    {
        if is_muted_actor(&db, &viewer.id, &actor.actor_uri).await? {
            continue;
        }
        entries.push((
            status.published_at.clone(),
            status.id.clone(),
            build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?,
        ));
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    let first_id = entries
        .first()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let last_id = entries
        .last()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let response = entries
        .into_iter()
        .map(|(_, _, value)| value)
        .take(limit as usize)
        .collect::<Vec<_>>();
    let mut builder = Response::from_json(&response)?;
    if let Some(link) =
        build_timeline_link_header(&req, limit, first_id.as_deref(), last_id.as_deref())?
    {
        builder.headers_mut().set("Link", &link)?;
    }
    Ok(builder)
}

pub(crate) async fn public_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: PublicTimelineQuery = req.query().unwrap_or_default();
    let limit = timeline_limit(&query.pagination);
    let query_limit = timeline_fetch_limit(limit);
    let include_local = include_local_source(query.local, query.remote);
    let include_remote = include_remote_source(query.local, query.remote);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match extract_authenticated_user(&req, &config).await? {
        Some(user) => Some(resolve_local_account(&db, &user).await?),
        None => None,
    };
    let cursor = resolve_timeline_cursor(&db, &query.pagination).await?;
    let mut entries = Vec::new();

    if include_local {
        for status in list_local_public_timeline_statuses(&db, &cursor, query_limit).await? {
            let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
                continue;
            };
            if let Some(viewer) = viewer.as_ref()
                && is_local_status_thread_muted_by(&db, &viewer.id, &status).await?
            {
                continue;
            }
            let media = find_media_attachments_by_status_id(&db, &status.id).await?;
            if query.only_media.unwrap_or(false) && media.is_empty() {
                continue;
            }
            entries.push((
                status.created_at.clone(),
                status.id.clone(),
                serde_json::to_value(
                    build_local_status_response(
                        &db,
                        &config,
                        viewer.as_ref(),
                        &status,
                        &account,
                        None,
                        media,
                    )
                    .await?,
                )
                .unwrap_or(serde_json::Value::Null),
            ));
        }
    }

    if include_remote {
        for (status, actor) in
            list_remote_public_timeline_statuses(&db, &cursor, query_limit).await?
        {
            if query.only_media.unwrap_or(false)
                && !remote_status_has_media(&db, &status.id).await?
            {
                continue;
            }
            entries.push((
                status.published_at.clone(),
                status.id.clone(),
                serde_json::to_value(
                    build_remote_status_response(&db, &config, viewer.as_ref(), &status, &actor)
                        .await?,
                )
                .unwrap_or(serde_json::Value::Null),
            ));
        }
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    let first_id = entries
        .first()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let last_id = entries
        .last()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let response = entries
        .into_iter()
        .map(|(_, _, value)| value)
        .take(limit as usize)
        .collect::<Vec<_>>();
    let mut builder = Response::from_json(&response)?;
    if let Some(link) =
        build_timeline_link_header(&req, limit, first_id.as_deref(), last_id.as_deref())?
    {
        builder.headers_mut().set("Link", &link)?;
    }
    Ok(builder)
}

pub(crate) async fn tag_timeline_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let tag = ctx
        .param("hashtag")
        .map(|value| normalize_hashtag(&value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing hashtag route parameter".to_owned()))?;
    let query: TagTimelineQuery = req.query().unwrap_or_default();
    let limit = timeline_limit(&query.pagination);
    let query_limit = timeline_fetch_limit(limit);
    let include_local = include_local_source(query.local, query.remote);
    let include_remote = include_remote_source(query.local, query.remote);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match extract_authenticated_user(&req, &config).await? {
        Some(user) => Some(resolve_local_account(&db, &user).await?),
        None => None,
    };
    let cursor = resolve_timeline_cursor(&db, &query.pagination).await?;
    let mut entries = Vec::new();

    if include_local {
        for status in list_local_public_statuses_by_tag(&db, &tag, &cursor, query_limit).await? {
            let status_tags = extract_hashtags_from_text(&status._text_content);
            if !matches_tag_timeline_filters(&status_tags, &tag, &query) {
                continue;
            }
            let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
                continue;
            };
            if let Some(viewer) = viewer.as_ref()
                && is_local_status_thread_muted_by(&db, &viewer.id, &status).await?
            {
                continue;
            }
            let media = find_media_attachments_by_status_id(&db, &status.id).await?;
            if query.only_media.unwrap_or(false) && media.is_empty() {
                continue;
            }
            entries.push((
                status.created_at.clone(),
                status.id.clone(),
                build_local_status_response(
                    &db,
                    &config,
                    viewer.as_ref(),
                    &status,
                    &account,
                    load_in_reply_to_account_id(&db, &status).await?,
                    media,
                )
                .await?,
            ));
        }
    }

    if include_remote {
        for (status, actor) in
            list_remote_public_statuses_by_tag(&db, &tag, &cursor, query_limit).await?
        {
            let status_tags = extract_hashtags_from_html(&status.content_html);
            if !matches_tag_timeline_filters(&status_tags, &tag, &query) {
                continue;
            }
            if query.only_media.unwrap_or(false)
                && !remote_status_has_media(&db, &status.id).await?
            {
                continue;
            }
            entries.push((
                status.published_at.clone(),
                status.id.clone(),
                build_remote_status_response(&db, &config, viewer.as_ref(), &status, &actor)
                    .await?,
            ));
        }
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    let first_id = entries
        .first()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let last_id = entries
        .last()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let response = entries
        .into_iter()
        .map(|(_, _, status)| status)
        .take(limit as usize)
        .collect::<Vec<_>>();
    let mut builder = Response::from_json(&response)?;
    if let Some(link) =
        build_timeline_link_header(&req, limit, first_id.as_deref(), last_id.as_deref())?
    {
        builder.headers_mut().set("Link", &link)?;
    }
    Ok(builder)
}
