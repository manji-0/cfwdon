use crate::auth::{
    LocalApiAuthentication, authenticate_local_api_request, extract_authenticated_user,
    find_account_by_id, find_authenticated_local_account, resolve_local_account,
};
use crate::content_helpers::{extract_hashtags_from_html, extract_hashtags_from_text};
use crate::find_media_attachments_by_status_id;
use crate::instance_identity::actor_url;
use crate::oauth_apps::{
    app_bearer_token_from_request, find_oauth_app_by_bearer_token,
    oauth_access_token_has_any_scope, oauth_app_has_any_scope,
};
use crate::runtime_config::load_config;
use crate::{
    HomeTimelineQuery, LinkTimelineQuery, PublicTimelineQuery, TagTimelineQuery,
    TimelinePaginationQuery, build_local_status_response, build_remote_status_response,
    build_status_card_value, build_timeline_link_header, canonicalize_link_timeline_url,
    derive_link_timeline_match_urls, enrich_card_with_remote_preview, include_local_source,
    include_remote_source, list_followed_tag_names, list_local_direct_timeline_statuses,
    list_local_home_timeline_statuses, list_local_public_statuses_by_link,
    list_local_public_statuses_by_tag, list_local_public_timeline_statuses,
    list_remote_home_timeline_statuses, list_remote_public_statuses_by_link,
    list_remote_public_statuses_by_tag, list_remote_public_timeline_statuses,
    load_in_reply_to_account_id, matches_tag_timeline_filters, normalize_hashtag,
    remote_status_has_media, resolve_timeline_cursor, strip_html_tags, timeline_fetch_limit,
    timeline_limit,
};
use crate::{is_local_status_thread_muted_by, is_muted_actor};
use cfwdon_core::TimelineAccessLevel;
use std::collections::HashSet;
use worker::D1Database;
use worker::{Error, Request, Response, Result, RouteContext};

pub(crate) enum TimelineRequestAccess {
    Viewer(crate::LocalAccount),
    ScopedApp,
    None,
    Invalid,
}

impl TimelineRequestAccess {
    fn viewer(&self) -> Option<&crate::LocalAccount> {
        match self {
            Self::Viewer(viewer) => Some(viewer),
            Self::ScopedApp | Self::None | Self::Invalid => None,
        }
    }

    fn is_authorized(&self) -> bool {
        matches!(self, Self::Viewer(_) | Self::ScopedApp)
    }
}

pub(crate) fn timeline_source_requires_authorization(level: TimelineAccessLevel) -> bool {
    !matches!(level, TimelineAccessLevel::Public)
}

pub(crate) fn timeline_request_requires_authorization(
    include_local: bool,
    include_remote: bool,
    local_access: TimelineAccessLevel,
    remote_access: TimelineAccessLevel,
) -> bool {
    (include_local && timeline_source_requires_authorization(local_access))
        || (include_remote && timeline_source_requires_authorization(remote_access))
}

pub(crate) async fn resolve_timeline_request_access(
    req: &Request,
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
) -> Result<TimelineRequestAccess> {
    if let Some(viewer) = find_authenticated_local_account(req, db, config).await? {
        return Ok(TimelineRequestAccess::Viewer(viewer));
    }

    let Some(token) = app_bearer_token_from_request(req)? else {
        return Ok(TimelineRequestAccess::None);
    };
    let Some(app) = find_oauth_app_by_bearer_token(db, &token).await? else {
        return Ok(TimelineRequestAccess::Invalid);
    };
    if oauth_app_has_any_scope(&app, &["read", "read:statuses"]) {
        Ok(TimelineRequestAccess::ScopedApp)
    } else {
        Ok(TimelineRequestAccess::Invalid)
    }
}

pub(crate) fn timeline_invalid_access_token_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "The access token is invalid",
    }))?
    .with_status(401))
}

pub(crate) fn timeline_outside_authorized_scopes_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "This action is outside the authorized scopes",
    }))?
    .with_status(403))
}

fn status_card_url_matches_targets(text: &str, targets: &HashSet<String>) -> bool {
    build_status_card_value(text)
        .and_then(|card| {
            card.get("url")
                .and_then(serde_json::Value::as_str)
                .and_then(canonicalize_link_timeline_url)
        })
        .map(|url| targets.contains(&url))
        .unwrap_or(false)
}

pub(crate) async fn home_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match authenticate_local_api_request(&req, &db, &config).await? {
        LocalApiAuthentication::OAuthToken(auth) => {
            if !oauth_access_token_has_any_scope(&auth.token, &["read:statuses", "read"]) {
                return timeline_outside_authorized_scopes_response();
            }
            auth.account
        }
        LocalApiAuthentication::Access(viewer) => viewer,
        LocalApiAuthentication::AppToken
        | LocalApiAuthentication::InvalidBearer
        | LocalApiAuthentication::None => return timeline_invalid_access_token_response(),
    };
    let query: HomeTimelineQuery = req.query().unwrap_or_default();
    let limit = timeline_limit(&query.pagination);
    let query_limit = timeline_fetch_limit(limit);
    let cursor = resolve_timeline_cursor(&db, &query.pagination).await?;
    let mut entries = Vec::new();
    let mut seen_status_ids = HashSet::new();

    for status in list_local_home_timeline_statuses(&db, &viewer.id, &cursor, query_limit).await? {
        if !seen_status_ids.insert(status.id.clone()) {
            continue;
        }
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
        if !seen_status_ids.insert(status.id.clone()) {
            continue;
        }
        if is_muted_actor(&db, &viewer.id, &actor.actor_uri).await? {
            continue;
        }
        entries.push((
            status.published_at.clone(),
            status.id.clone(),
            build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?,
        ));
    }

    for tag in list_followed_tag_names(&db, &viewer.id).await? {
        for status in list_local_public_statuses_by_tag(&db, &tag, &cursor, query_limit).await? {
            if !seen_status_ids.insert(status.id.clone()) {
                continue;
            }
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
            list_remote_public_statuses_by_tag(&db, &tag, &cursor, query_limit).await?
        {
            if !seen_status_ids.insert(status.id.clone()) {
                continue;
            }
            if is_muted_actor(&db, &viewer.id, &actor.actor_uri).await? {
                continue;
            }
            entries.push((
                status.published_at.clone(),
                status.id.clone(),
                build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?,
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
    let access = resolve_timeline_request_access(&req, &db, &config).await?;
    if timeline_request_requires_authorization(
        include_local,
        include_remote,
        config.timeline_live_feeds_local,
        config.timeline_live_feeds_remote,
    ) && !access.is_authorized()
    {
        return timeline_invalid_access_token_response();
    }
    let viewer = access.viewer();
    let cursor = resolve_timeline_cursor(&db, &query.pagination).await?;
    let mut entries = Vec::new();

    if include_local {
        for status in list_local_public_timeline_statuses(&db, &cursor, query_limit).await? {
            let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
                continue;
            };
            if let Some(viewer) = viewer
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
                        &db, &config, viewer, &status, &account, None, media,
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
                    build_remote_status_response(&db, &config, viewer, &status, &actor).await?,
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
    let access = resolve_timeline_request_access(&req, &db, &config).await?;
    if timeline_request_requires_authorization(
        include_local,
        include_remote,
        config.timeline_hashtag_feeds_local,
        config.timeline_hashtag_feeds_remote,
    ) && !access.is_authorized()
    {
        return timeline_invalid_access_token_response();
    }
    let viewer = access.viewer();
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
            if let Some(viewer) = viewer
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
                    viewer,
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
                build_remote_status_response(&db, &config, viewer, &status, &actor).await?,
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

pub(crate) async fn link_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: LinkTimelineQuery = req.query().unwrap_or_default();
    let target_url = query
        .url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing url query parameter".to_owned()))?;
    let target_urls = derive_link_timeline_match_urls(target_url);
    let limit = timeline_limit(&query.pagination);
    let query_limit = timeline_fetch_limit(limit);
    let db = ctx.d1(&config.database_binding)?;
    let access = resolve_timeline_request_access(&req, &db, &config).await?;
    if timeline_request_requires_authorization(
        true,
        true,
        config.timeline_trending_link_feeds_local,
        config.timeline_trending_link_feeds_remote,
    ) && !access.is_authorized()
    {
        return timeline_invalid_access_token_response();
    }
    if !crate::trending_link_target_is_known(&db, &target_urls).await? {
        return Response::error("Record not found", 404);
    }
    let target_url_set = target_urls
        .iter()
        .filter_map(|url| canonicalize_link_timeline_url(&url))
        .collect::<HashSet<_>>();
    let viewer = access.viewer();
    let cursor = resolve_timeline_cursor(&db, &query.pagination).await?;
    let mut entries = Vec::new();

    for status in
        list_local_public_statuses_by_link(&db, &target_urls, &cursor, query_limit).await?
    {
        if !status_card_url_matches_targets(&status._text_content, &target_url_set) {
            continue;
        }
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            continue;
        };
        if let Some(viewer) = viewer
            && is_local_status_thread_muted_by(&db, &viewer.id, &status).await?
        {
            continue;
        }
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        let mut value = serde_json::to_value(
            build_local_status_response(
                &db,
                &config,
                viewer,
                &status,
                &account,
                load_in_reply_to_account_id(&db, &status).await?,
                media,
            )
            .await?,
        )
        .unwrap_or(serde_json::Value::Null);
        if let Some(card) = value.get_mut("card") {
            let _ = enrich_card_with_remote_preview(card).await;
        }
        entries.push((status.created_at.clone(), status.id.clone(), value));
    }

    for (status, actor) in
        list_remote_public_statuses_by_link(&db, &target_urls, &cursor, query_limit).await?
    {
        if !status_card_url_matches_targets(&strip_html_tags(&status.content_html), &target_url_set)
        {
            continue;
        }
        let mut value = serde_json::to_value(
            build_remote_status_response(&db, &config, viewer, &status, &actor).await?,
        )
        .unwrap_or(serde_json::Value::Null);
        if let Some(card) = value.get_mut("card") {
            let _ = enrich_card_with_remote_preview(card).await;
        }
        entries.push((status.published_at.clone(), status.id.clone(), value));
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    if entries.is_empty() {
        return Response::error("Record not found", 404);
    }
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

#[cfg(test)]
mod tests {
    use super::{
        status_card_url_matches_targets, timeline_request_requires_authorization,
        timeline_source_requires_authorization,
    };
    use cfwdon_core::TimelineAccessLevel;
    use std::collections::HashSet;

    #[test]
    fn timeline_source_requires_authorization_for_non_public_levels() {
        assert!(!timeline_source_requires_authorization(
            TimelineAccessLevel::Public
        ));
        assert!(timeline_source_requires_authorization(
            TimelineAccessLevel::Authenticated
        ));
        assert!(timeline_source_requires_authorization(
            TimelineAccessLevel::Disabled
        ));
    }

    #[test]
    fn timeline_request_requires_authorization_only_for_requested_sources() {
        assert!(!timeline_request_requires_authorization(
            true,
            false,
            TimelineAccessLevel::Public,
            TimelineAccessLevel::Authenticated,
        ));
        assert!(timeline_request_requires_authorization(
            true,
            true,
            TimelineAccessLevel::Public,
            TimelineAccessLevel::Authenticated,
        ));
        assert!(timeline_request_requires_authorization(
            false,
            true,
            TimelineAccessLevel::Public,
            TimelineAccessLevel::Disabled,
        ));
    }

    #[test]
    fn status_card_url_matches_targets_uses_primary_card_url() {
        let targets = ["https://example.com/articles/rust".to_owned()]
            .into_iter()
            .collect::<HashSet<_>>();

        assert!(status_card_url_matches_targets(
            "see https://example.com/articles/rust and https://example.com/other",
            &targets,
        ));
        assert!(!status_card_url_matches_targets(
            "see https://example.com/other and https://example.com/articles/rust",
            &targets,
        ));
    }
}

pub(crate) async fn direct_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query: TimelinePaginationQuery = req.query().unwrap_or_default();
    let limit = timeline_limit(&query);
    let query_limit = timeline_fetch_limit(limit);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = resolve_local_account(&db, &user).await?;
    let cursor = resolve_timeline_cursor(&db, &query).await?;
    let mut entries = Vec::new();

    for status in list_local_direct_timeline_statuses(&db, &viewer.id, &cursor, query_limit).await?
    {
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
