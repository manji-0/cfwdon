use crate::auth::{extract_authenticated_user, find_account_by_id, resolve_local_account};
use crate::content_helpers::{extract_hashtags_from_html, extract_hashtags_from_text};
use crate::find_media_attachments_by_status_id;
use crate::instance_identity::actor_url;
use crate::is_muted_actor;
use crate::runtime_config::load_config;
use crate::{
    HomeTimelineQuery, TagTimelineQuery, build_local_status_response, build_remote_status_response,
    include_local_source, include_remote_source, list_local_home_timeline_statuses,
    list_local_public_statuses_by_tag, list_local_public_timeline_statuses,
    list_remote_home_timeline_statuses, list_remote_public_statuses_by_tag,
    list_remote_public_timeline_statuses, load_in_reply_to_account_id,
    matches_tag_timeline_filters, normalize_hashtag,
};
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
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = resolve_local_account(&db, &user).await?;
    let mut entries = Vec::new();

    for status in
        list_local_home_timeline_statuses(&db, &viewer.id, limit.saturating_mul(3)).await?
    {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            continue;
        };
        if is_muted_actor(&db, &viewer.id, &actor_url(&config, &account.username)).await? {
            continue;
        }
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        entries.push((
            status.created_at.clone(),
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
        list_remote_home_timeline_statuses(&db, &viewer.id, limit.saturating_mul(3)).await?
    {
        if is_muted_actor(&db, &viewer.id, &actor.actor_uri).await? {
            continue;
        }
        entries.push((
            status.published_at.clone(),
            build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?,
        ));
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0));
    Response::from_json(
        &entries
            .into_iter()
            .map(|(_, value)| value)
            .take(limit as usize)
            .collect::<Vec<_>>(),
    )
}

pub(crate) async fn public_timeline_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let mut entries = Vec::new();

    for status in list_local_public_timeline_statuses(&db, 20).await? {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            continue;
        };
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        entries.push((
            status.created_at.clone(),
            serde_json::to_value(
                build_local_status_response(&db, &config, None, &status, &account, None, media)
                    .await?,
            )
            .unwrap_or(serde_json::Value::Null),
        ));
    }

    for (status, actor) in list_remote_public_timeline_statuses(&db, 20).await? {
        entries.push((
            status.published_at.clone(),
            serde_json::to_value(
                build_remote_status_response(&db, &config, None, &status, &actor).await?,
            )
            .unwrap_or(serde_json::Value::Null),
        ));
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0));
    let response = entries
        .into_iter()
        .map(|(_, value)| value)
        .take(20)
        .collect::<Vec<_>>();

    Response::from_json(&response)
}

pub(crate) async fn tag_timeline_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let tag = ctx
        .param("hashtag")
        .map(|value| normalize_hashtag(&value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing hashtag route parameter".to_owned()))?;
    let query: TagTimelineQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let query_limit = limit.saturating_mul(4).clamp(limit, 160);
    let include_local = include_local_source(query.local, query.remote);
    let include_remote = include_remote_source(query.local, query.remote);
    let db = ctx.d1(&config.database_binding)?;
    let mut entries = Vec::new();

    if include_local {
        for status in list_local_public_statuses_by_tag(&db, &tag, query_limit).await? {
            let status_tags = extract_hashtags_from_text(&status._text_content);
            if !matches_tag_timeline_filters(&status_tags, &tag, &query) {
                continue;
            }
            let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
                continue;
            };
            let media = find_media_attachments_by_status_id(&db, &status.id).await?;
            if query.only_media.unwrap_or(false) && media.is_empty() {
                continue;
            }
            entries.push((
                status.created_at.clone(),
                build_local_status_response(
                    &db,
                    &config,
                    None,
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
        for (status, actor) in list_remote_public_statuses_by_tag(&db, &tag, query_limit).await? {
            let status_tags = extract_hashtags_from_html(&status.content_html);
            if !matches_tag_timeline_filters(&status_tags, &tag, &query) {
                continue;
            }
            if query.only_media.unwrap_or(false) {
                continue;
            }
            entries.push((
                status.published_at.clone(),
                build_remote_status_response(&db, &config, None, &status, &actor).await?,
            ));
        }
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0));
    Response::from_json(
        &entries
            .into_iter()
            .map(|(_, status)| status)
            .take(limit as usize)
            .collect::<Vec<_>>(),
    )
}
