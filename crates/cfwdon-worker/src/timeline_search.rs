use crate::auth::{LocalApiAuthentication, authenticate_local_api_request};
use crate::responses::MastodonSearchResponse;
use crate::runtime_config::load_config;
use crate::tags::{resolve_search_tag, search_tags_for_v2};
use crate::{
    LocalAccount, SearchCategoryFlags, SearchUrlQueryMode, SearchV2Query,
    account_search_non_exact_limit, effective_search_v2_following, effective_search_v2_offset,
    normalize_search_query_input, oauth_access_token_has_any_scope,
    resolve_cached_exact_search_account, resolve_search_account, resolve_search_status,
    search_cached_accounts, search_category_flags, search_statuses_for_v2, search_v2_limit,
    search_v2_requires_auth, search_v2_type_allows_url_resource, search_v2_unauthenticated_error,
    search_v2_url_query_mode,
};
use worker::{Request, Response, Result, RouteContext};

pub(crate) async fn search_v2(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let response = match search_impl(&req, &ctx).await {
        Ok(response) => response,
        Err(response) => return Ok(response),
    };
    Response::from_json(&response)
}

pub(crate) async fn search_v1(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let response = match search_impl(&req, &ctx).await {
        Ok(response) => response,
        Err(response) => return Ok(response),
    };
    let mut value = serde_json::to_value(response).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize search response: {error}"))
    })?;

    if let Some(object) = value.as_object_mut() {
        if let Some(hashtags) = object
            .get_mut("hashtags")
            .and_then(serde_json::Value::as_array_mut)
        {
            let names = hashtags
                .drain(..)
                .filter_map(|value| match value {
                    serde_json::Value::String(name) => Some(serde_json::Value::String(name)),
                    serde_json::Value::Object(object) => object
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .map(|name| serde_json::Value::String(name.to_owned())),
                    _ => None,
                })
                .collect::<Vec<_>>();
            *hashtags = names;
        }
        object.remove("collections");
    }

    Response::from_json(&value)
}

async fn search_impl(
    req: &Request,
    ctx: &RouteContext<()>,
) -> std::result::Result<MastodonSearchResponse, Response> {
    let config = load_config(ctx);
    let query: SearchV2Query = req.query().unwrap_or_default();
    let normalized_query = normalize_search_query_input(&query.q);
    let q = normalized_query.trim();
    if q.is_empty() {
        return Ok(MastodonSearchResponse::default());
    }

    let db = match ctx.d1(&config.database_binding) {
        Ok(db) => db,
        Err(error) => return Err(Response::error(error.to_string(), 500).unwrap()),
    };
    let requires_auth = search_v2_requires_auth(&query);
    let viewer = match authenticate_local_api_request(req, &db, &config)
        .await
        .map_err(|error| Response::error(error.to_string(), 500).unwrap())?
    {
        LocalApiAuthentication::Access(account) => Some(account),
        LocalApiAuthentication::OAuthToken(auth) => {
            if !oauth_access_token_has_any_scope(&auth.token, &["read:search", "read"]) {
                return Err(
                    Response::error("This action is outside the authorized scopes", 403).unwrap(),
                );
            }
            Some(auth.account)
        }
        LocalApiAuthentication::AppToken | LocalApiAuthentication::InvalidBearer => {
            return Err(Response::error("The access token is invalid", 401).unwrap());
        }
        LocalApiAuthentication::None => {
            if requires_auth {
                return Err(Response::error(
                    search_v2_unauthenticated_error(&query)
                        .unwrap_or("Cloudflare Access authentication required"),
                    401,
                )
                .unwrap());
            }
            None
        }
    };

    let search_flags: SearchCategoryFlags = search_category_flags(query.search_type.as_deref());
    let limit = search_v2_limit(query.limit);
    let offset = effective_search_v2_offset(&query);
    let resolve_enabled = query.resolve.unwrap_or(false);
    match search_v2_url_query_mode(q, resolve_enabled, offset) {
        SearchUrlQueryMode::None => {}
        SearchUrlQueryMode::EmptyResults => {
            return Ok(MastodonSearchResponse::default());
        }
        SearchUrlQueryMode::ResolveOnly => {
            return resolve_search_url_only_response(
                &db,
                &config,
                viewer.as_ref(),
                q,
                query.search_type.as_deref(),
                search_flags,
            )
            .await
            .map_err(|error| Response::error(error.to_string(), 500).unwrap());
        }
    }

    let mut response = MastodonSearchResponse::default();

    if search_flags.accounts {
        let following_only = effective_search_v2_following(&query, viewer.is_some());
        let exact_account = if offset == 0 {
            resolve_cached_exact_search_account(&db, &config, viewer.as_ref(), q, following_only)
                .await
                .map_err(|error| Response::error(error.to_string(), 500).unwrap())?
        } else {
            None
        };
        let non_exact_limit =
            account_search_non_exact_limit(q, viewer.as_ref(), limit, exact_account.is_some());
        response.accounts = if non_exact_limit == 0 {
            Vec::new()
        } else {
            search_cached_accounts(
                &db,
                &config,
                viewer.as_ref(),
                q,
                non_exact_limit,
                offset,
                following_only,
            )
            .await
            .map_err(|error| Response::error(error.to_string(), 500).unwrap())?
        };
        if let Some(account) = exact_account
            && !response
                .accounts
                .iter()
                .any(|candidate| candidate.id == account.id)
        {
            response.accounts.insert(0, account);
        }
        response.accounts.truncate(limit as usize);
        if resolve_enabled
            && response.accounts.is_empty()
            && let Some(account) = resolve_search_account(&db, &config, q)
                .await
                .map_err(|error| Response::error(error.to_string(), 500).unwrap())?
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
            query.max_id.as_deref(),
            query.min_id.as_deref(),
        )
        .await
        .map_err(|error| Response::error(error.to_string(), 500).unwrap())?;
        if resolve_enabled
            && response.statuses.is_empty()
            && let Some(status) = resolve_search_status(&db, &config, viewer, q)
                .await
                .map_err(|error| Response::error(error.to_string(), 500).unwrap())?
        {
            response.statuses.push(status);
            response.statuses.truncate(limit as usize);
        }
    }

    if search_flags.hashtags {
        response.hashtags = search_tags_for_v2(&db, &config, q, limit, offset)
            .await
            .map_err(|error| Response::error(error.to_string(), 500).unwrap())?;
        if resolve_enabled
            && response.hashtags.is_empty()
            && let Some(tag) = resolve_search_tag(&db, &config, q)
                .await
                .map_err(|error| Response::error(error.to_string(), 500).unwrap())?
        {
            response.hashtags.push(tag);
            response.hashtags.truncate(limit as usize);
        }
    }

    Ok(response)
}

async fn resolve_search_url_only_response(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&LocalAccount>,
    query: &str,
    search_type: Option<&str>,
    search_flags: SearchCategoryFlags,
) -> Result<MastodonSearchResponse> {
    let mut response = MastodonSearchResponse::default();

    if let Some(viewer) = viewer
        && let Some(status) = resolve_search_status(db, config, viewer, query).await?
    {
        if search_flags.statuses && search_v2_type_allows_url_resource(search_type, "statuses") {
            response.statuses.push(status);
        }
        return Ok(response);
    }

    if let Some(account) = resolve_search_account(db, config, query).await? {
        if search_flags.accounts && search_v2_type_allows_url_resource(search_type, "accounts") {
            response.accounts.push(account);
        }
        return Ok(response);
    }

    if let Some(tag) = resolve_search_tag(db, config, query).await? {
        if search_flags.hashtags && search_v2_type_allows_url_resource(search_type, "hashtags") {
            response.hashtags.push(tag);
        }
    }

    Ok(response)
}
