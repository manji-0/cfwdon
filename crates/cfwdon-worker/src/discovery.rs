use crate::{
    AppConfig, CACHE_TTL_FEDERATION, CACHE_TTL_TRENDS, Error, Request, Response, Result,
    RouteContext, actor_url, build_activitypub_actor_document, build_tag_response,
    cache_actor_json_response, cache_actor_profile_html_response, cache_public_json_response,
    cache_public_response, cache_public_response_with_options, cached_actor_json_response,
    cached_actor_profile_html_response, ensure_account_keys, find_account_by_username,
    find_media_attachments_by_status_ids, instance_base_url, instance_host,
    list_public_outbox_statuses, load_account_stats, load_config, local_status_html_item,
    normalize_hashtag,
};

mod collections;
mod host_meta;
mod profile_html;
mod remote_follow;
mod webfinger;

pub(crate) use collections::*;
pub(crate) use host_meta::*;
pub(in crate::discovery) use profile_html::{profile_html_document, profile_html_response};
pub(crate) use remote_follow::*;
pub(crate) use webfinger::*;

pub(crate) async fn actor_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let username = route_username(&ctx)?;

    let wants_html = prefers_profile_html(&req)?;
    if wants_html {
        if let Some(response) = cached_actor_profile_html_response(&ctx, &username).await? {
            return with_profile_discovery_link_headers(response, &config, &username);
        }
    } else if let Some(response) = cached_actor_json_response(&ctx, &username).await? {
        return Ok(response);
    }

    let db = crate::bind_request_d1(&ctx, &config)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let account = ensure_account_keys(&db, &config, account).await?;

    if wants_html {
        let stats = load_account_stats(&db, account.id()).await?;
        let statuses = list_public_outbox_statuses(&db, account.id(), 20).await?;
        let status_ids = statuses
            .iter()
            .map(|status| status.id.clone())
            .collect::<Vec<_>>();
        let mut media_by_status_id = find_media_attachments_by_status_ids(&db, &status_ids).await?;
        let posts_html = statuses
            .iter()
            .map(|status| {
                let media = media_by_status_id.remove(&status.id).unwrap_or_default();
                local_status_html_item(&config, &account, status, &media)
            })
            .collect::<Vec<_>>()
            .join("");
        let html = profile_html_document(&config, &account, &stats, &posts_html);
        cache_actor_profile_html_response(&ctx, &username, html.clone()).await?;
        let cache_tag = format!("account-{username}");
        let response = cache_public_response_with_options(
            profile_html_response(html)?,
            CACHE_TTL_FEDERATION,
            None,
            &[("Cache-Tag", &cache_tag)],
        )?;
        return with_profile_discovery_link_headers(response, &config, &username);
    }

    let response = build_activitypub_actor_document(&config, &account);
    cache_actor_json_response(&ctx, &username, &response).await?;

    let cache_tag = format!("account-{username}");
    cache_public_json_response(
        &response,
        "application/activity+json",
        CACHE_TTL_FEDERATION,
        &[("Vary", "Accept"), ("Cache-Tag", &cache_tag)],
    )
}

fn route_username(ctx: &RouteContext<()>) -> Result<String> {
    ctx.param("username")
        .map(|value| value.trim().trim_start_matches('@').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))
}

fn with_profile_discovery_link_headers(
    mut response: Response,
    config: &AppConfig,
    username: &str,
) -> Result<Response> {
    let acct = format!("acct:{}@{}", username, instance_host(config));
    let webfinger = format!(
        "{}/.well-known/webfinger?resource={}",
        instance_base_url(config),
        urlencoding::encode(&acct)
    );
    let actor = actor_url(config, username);
    let link = format!(
        "<{webfinger}>; rel=\"lrdd\"; type=\"application/jrd+json\", <{actor}>; rel=\"alternate\"; type=\"application/activity+json\""
    );
    response.headers_mut().set("Link", &link)?;
    if response.headers().get("Vary")?.is_none() {
        response.headers_mut().set("Vary", "Accept")?;
    }
    Ok(response)
}

fn prefers_profile_html(req: &Request) -> Result<bool> {
    let accept = req.headers().get("Accept")?.unwrap_or_default();
    let accept = accept.to_ascii_lowercase();
    Ok(accept.contains("text/html")
        && !accept.contains("application/activity+json")
        && !accept.contains("application/ld+json"))
}

pub(crate) async fn tag_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let tag = ctx
        .param("name")
        .or_else(|| ctx.param("hashtag"))
        .map(|value| normalize_hashtag(value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing tag route parameter".to_owned()))?;
    let db = crate::bind_request_d1(&ctx, &config)?;

    cache_public_response(
        Response::from_json(&build_tag_response(&db, &config, &tag).await?)?,
        CACHE_TTL_TRENDS,
    )
}
