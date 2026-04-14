use crate::auth::find_authenticated_local_account;
use crate::responses::MastodonSearchResponse;
use crate::runtime_config::load_config;
use crate::tags::{resolve_search_tag, search_tags_for_v2};
use crate::{
    SearchCategoryFlags, SearchV2Query, resolve_search_account, resolve_search_status,
    search_cached_accounts, search_category_flags, search_statuses_for_v2, search_v2_limit,
    search_v2_requires_auth,
};
use worker::{Request, Response, Result, RouteContext};

pub(crate) async fn search_v2(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: SearchV2Query = req.query().unwrap_or_default();
    let q = query.q.trim();
    if q.is_empty() {
        return Response::from_json(&MastodonSearchResponse::default());
    }

    let db = ctx.d1(&config.database_binding)?;
    let requires_auth = search_v2_requires_auth(&query);
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => Some(account),
        None if requires_auth => {
            return Response::error("Cloudflare Access authentication required", 401);
        }
        None => None,
    };

    let search_flags: SearchCategoryFlags = search_category_flags(query.search_type.as_deref());
    let limit = search_v2_limit(query.limit);
    let offset = query.offset.unwrap_or(0);
    let resolve_enabled = query.resolve.unwrap_or(false);
    let mut response = MastodonSearchResponse::default();

    if search_flags.accounts {
        response.accounts = search_cached_accounts(
            &db,
            &config,
            viewer.as_ref(),
            q,
            limit,
            offset,
            query.following.unwrap_or(false),
        )
        .await?;
        if resolve_enabled
            && response.accounts.is_empty()
            && let Some(account) = resolve_search_account(&db, &config, q).await?
        {
            response.accounts.push(account);
            response.accounts.truncate(limit as usize);
        }
    }

    if search_flags.statuses
        && let Some(viewer) = viewer.as_ref()
    {
        response.statuses = search_statuses_for_v2(
            &db,
            &config,
            viewer,
            q,
            limit,
            offset,
            query.account_id.as_deref(),
        )
        .await?;
        if resolve_enabled
            && response.statuses.is_empty()
            && let Some(status) = resolve_search_status(&db, &config, viewer, q).await?
        {
            response.statuses.push(status);
            response.statuses.truncate(limit as usize);
        }
    }

    if search_flags.hashtags {
        response.hashtags = search_tags_for_v2(&db, &config, q, limit).await?;
        if resolve_enabled
            && response.hashtags.is_empty()
            && let Some(tag) = resolve_search_tag(&db, &config, q).await?
        {
            response.hashtags.push(tag);
            response.hashtags.truncate(limit as usize);
        }
    }

    Response::from_json(&response)
}
