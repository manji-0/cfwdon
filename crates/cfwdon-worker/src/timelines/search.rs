use crate::auth::{LocalApiAuthentication, authenticate_local_api_request};
use crate::responses::MastodonSearchResponse;
use crate::runtime_config::load_config;
use crate::tags::{resolve_search_tag, search_tags_for_v2};
use crate::{
    LocalAccount, MastodonAccountResponse, MastodonStatusResponse, MastodonTagResponse,
    SearchCategoryFlags, SearchUrlQueryMode, SearchV2ExecutionPlan, SearchV2Query,
    account_search_non_exact_limit, effective_search_v2_following,
    oauth_access_token_has_any_scope, resolve_cached_exact_search_account, resolve_search_account,
    resolve_search_status, search_cached_accounts, search_statuses_for_v2,
    search_v2_type_allows_url_resource, search_v2_unauthenticated_error, search_v2_url_query_mode,
};
use worker::{Request, Response, ResponseBody, Result, RouteContext};

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
    Response::from_json(&search_v1_legacy_response_value(response)?)
}

fn search_v1_legacy_response_value(response: MastodonSearchResponse) -> Result<serde_json::Value> {
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
                .filter_map(search_v1_hashtag_name)
                .collect();
            *hashtags = names;
        }
        object.remove("collections");
    }

    Ok(value)
}

fn search_v1_hashtag_name(value: serde_json::Value) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::String(name) => Some(serde_json::Value::String(name)),
        serde_json::Value::Object(object) => object
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(|name| serde_json::Value::String(name.to_owned())),
        _ => None,
    }
}

async fn search_impl(
    req: &Request,
    ctx: &RouteContext<()>,
) -> std::result::Result<MastodonSearchResponse, Response> {
    let config = load_config(ctx);
    let plan = SearchV2ExecutionPlan::from_query(req.query().unwrap_or_default());
    let query = &plan.query;
    let q = plan.query_text();
    if q.is_empty() {
        return Ok(MastodonSearchResponse::default());
    }

    let db = search_database_from_context(ctx, &config)?;
    let viewer = authenticate_search_viewer(req, &db, &config, &plan).await?;

    if let Some(response) =
        search_url_mode_response(&db, &config, viewer.as_ref(), q, &plan).await?
    {
        return Ok(response);
    }

    search_standard_response(&db, &config, viewer.as_ref(), q, query, &plan).await
}

async fn search_url_mode_response(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&LocalAccount>,
    query_text: &str,
    plan: &SearchV2ExecutionPlan,
) -> std::result::Result<Option<MastodonSearchResponse>, Response> {
    match search_v2_url_query_mode(query_text, plan.resolve_enabled, plan.offset) {
        SearchUrlQueryMode::None => Ok(None),
        SearchUrlQueryMode::EmptyResults => Ok(Some(MastodonSearchResponse::default())),
        SearchUrlQueryMode::ResolveOnly => search_worker_result(
            resolve_search_url_only_response(
                db,
                config,
                viewer,
                query_text,
                plan.query.search_type.as_deref(),
                plan.search_flags,
            )
            .await,
        )
        .map(Some),
    }
}

async fn search_standard_response(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&LocalAccount>,
    query_text: &str,
    query: &SearchV2Query,
    plan: &SearchV2ExecutionPlan,
) -> std::result::Result<MastodonSearchResponse, Response> {
    let mut response = MastodonSearchResponse::default();

    if plan.search_flags.accounts {
        response.accounts = search_worker_result(
            search_accounts_for_response(db, config, viewer, query_text, query, plan).await,
        )?;
    }

    if plan.search_flags.statuses {
        response.statuses = search_worker_result(
            search_statuses_for_response(db, config, viewer, query_text, query, plan).await,
        )?;
    }

    if plan.search_flags.hashtags {
        response.hashtags =
            search_worker_result(search_hashtags_for_response(db, config, query_text, plan).await)?;
    }

    Ok(response)
}

fn search_database_from_context(
    ctx: &RouteContext<()>,
    config: &cfwdon_core::AppConfig,
) -> std::result::Result<worker::D1Database, Response> {
    ctx.d1(&config.database_binding)
        .map_err(|error| search_error_response(error.to_string(), 500))
}

async fn authenticate_search_viewer(
    req: &Request,
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    plan: &SearchV2ExecutionPlan,
) -> std::result::Result<Option<LocalAccount>, Response> {
    match authenticate_local_api_request(req, db, config)
        .await
        .map_err(|error| search_error_response(error.to_string(), 500))?
    {
        LocalApiAuthentication::Access(account) => Ok(Some(account)),
        LocalApiAuthentication::OAuthToken(auth) => {
            if !oauth_access_token_has_any_scope(&auth.token, &["read:search", "read"]) {
                return Err(search_error_response(
                    "This action is outside the authorized scopes",
                    403,
                ));
            }
            Ok(Some(auth.account))
        }
        LocalApiAuthentication::AppToken | LocalApiAuthentication::InvalidBearer => {
            Err(search_error_response("The access token is invalid", 401))
        }
        LocalApiAuthentication::None => unauthenticated_search_viewer(plan),
    }
}

fn unauthenticated_search_viewer(
    plan: &SearchV2ExecutionPlan,
) -> std::result::Result<Option<LocalAccount>, Response> {
    if let Some(message) = unauthenticated_search_viewer_error_message(plan) {
        return Err(search_error_response(message, 401));
    }

    Ok(None)
}

fn unauthenticated_search_viewer_error_message(
    plan: &SearchV2ExecutionPlan,
) -> Option<&'static str> {
    plan.requires_auth.then(|| {
        search_v2_unauthenticated_error(&plan.query)
            .unwrap_or("Cloudflare Access authentication required")
    })
}

fn search_error_response(message: impl ToString, status: u16) -> Response {
    Response::error(message.to_string(), status).unwrap_or_else(|_| {
        Response::from_body(ResponseBody::Body(Vec::new()))
            .expect("empty response body must be constructible")
            .with_status(status)
    })
}

fn search_worker_result<T>(result: Result<T>) -> std::result::Result<T, Response> {
    result.map_err(|error| search_error_response(error.to_string(), 500))
}

async fn search_accounts_for_response(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&LocalAccount>,
    query_text: &str,
    query: &SearchV2Query,
    plan: &SearchV2ExecutionPlan,
) -> Result<Vec<MastodonAccountResponse>> {
    let following_only = effective_search_v2_following(query, viewer.is_some());
    let exact_account = if plan.offset == 0 {
        resolve_cached_exact_search_account(db, config, viewer, query_text, following_only).await?
    } else {
        None
    };
    let non_exact_limit =
        account_search_non_exact_limit(query_text, viewer, plan.limit, exact_account.is_some());
    let mut accounts = if non_exact_limit == 0 {
        Vec::new()
    } else {
        search_cached_accounts(
            db,
            config,
            viewer,
            query_text,
            non_exact_limit,
            plan.offset,
            following_only,
        )
        .await?
    };
    if let Some(account) = exact_account
        && !accounts.iter().any(|candidate| candidate.id == account.id)
    {
        accounts.insert(0, account);
    }
    accounts.truncate(plan.limit as usize);
    if plan.resolve_enabled
        && accounts.is_empty()
        && let Some(account) = resolve_search_account(db, config, query_text).await?
    {
        accounts.push(account);
        accounts.truncate(plan.limit as usize);
    }
    Ok(accounts)
}

async fn search_statuses_for_response(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&LocalAccount>,
    query_text: &str,
    query: &SearchV2Query,
    plan: &SearchV2ExecutionPlan,
) -> Result<Vec<MastodonStatusResponse>> {
    let Some(viewer) = viewer else {
        return Ok(Vec::new());
    };
    let mut statuses = search_statuses_for_v2(
        db,
        config,
        viewer,
        query_text,
        plan.limit,
        plan.offset,
        query.account_id.as_deref(),
        query.max_id.as_deref(),
        query.min_id.as_deref(),
    )
    .await?;
    if plan.resolve_enabled
        && statuses.is_empty()
        && let Some(status) = resolve_search_status(db, config, viewer, query_text).await?
    {
        statuses.push(status);
        statuses.truncate(plan.limit as usize);
    }
    Ok(statuses)
}

async fn search_hashtags_for_response(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    query_text: &str,
    plan: &SearchV2ExecutionPlan,
) -> Result<Vec<MastodonTagResponse>> {
    let mut hashtags = search_tags_for_v2(db, config, query_text, plan.limit, plan.offset).await?;
    if plan.resolve_enabled
        && hashtags.is_empty()
        && let Some(tag) = resolve_search_tag(db, config, query_text).await?
    {
        hashtags.push(tag);
        hashtags.truncate(plan.limit as usize);
    }
    Ok(hashtags)
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

    if let Some(tag) = resolve_search_tag(db, config, query).await?
        && search_flags.hashtags
        && search_v2_type_allows_url_resource(search_type, "hashtags")
    {
        response.hashtags.push(tag);
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_v1_hashtag_name_accepts_string_or_tag_object() {
        assert_eq!(
            search_v1_hashtag_name(serde_json::json!("rust")),
            Some(serde_json::json!("rust"))
        );
        assert_eq!(
            search_v1_hashtag_name(serde_json::json!({
                "name": "cfwdon",
                "url": "https://example.test/tags/cfwdon"
            })),
            Some(serde_json::json!("cfwdon"))
        );
        assert_eq!(search_v1_hashtag_name(serde_json::json!(123)), None);
    }

    #[test]
    fn search_v1_legacy_response_removes_collections_and_flattens_hashtags() {
        let response = MastodonSearchResponse {
            hashtags: vec![MastodonTagResponse {
                id: "tag-1".to_owned(),
                name: "rust".to_owned(),
                url: "https://example.test/tags/rust".to_owned(),
                history: Vec::new(),
                following: false,
                featured: false,
            }],
            collections: vec![serde_json::json!({"id": "collection-1"})],
            ..MastodonSearchResponse::default()
        };

        let value = search_v1_legacy_response_value(response).unwrap();

        assert!(value.get("collections").is_none());
        assert_eq!(value["hashtags"], serde_json::json!(["rust"]));
    }

    #[test]
    fn unauthenticated_search_viewer_allows_plain_query() {
        let plan = SearchV2ExecutionPlan::from_query(SearchV2Query {
            q: "rust".to_owned(),
            ..SearchV2Query::default()
        });

        match unauthenticated_search_viewer(&plan) {
            Ok(viewer) => assert!(viewer.is_none()),
            Err(_) => panic!("plain search should allow anonymous viewers"),
        }
    }

    #[test]
    fn unauthenticated_search_viewer_rejects_remote_resolution() {
        let plan = SearchV2ExecutionPlan::from_query(SearchV2Query {
            q: "https://remote.example/@alice".to_owned(),
            resolve: Some(true),
            ..SearchV2Query::default()
        });

        assert_eq!(
            unauthenticated_search_viewer_error_message(&plan),
            Some(
                "Search queries that resolve remote resources are not supported without authentication"
            )
        );
    }
}
