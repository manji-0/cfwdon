use super::{
    MastodonAccountResponse, Request, Response, Result, RouteContext,
    build_internal_cursor_link_header, extract_authenticated_user, find_account_by_id,
    find_authenticated_local_account, find_remote_actor_by_actor_uri,
    list_familiar_local_accounts_for_local_target, list_familiar_local_accounts_for_remote_target,
    list_familiar_remote_actors_for_local_target, list_mutes_for_account, load_account_stats,
    load_config, parse_internal_pagination_id, remote_account_rest_id, resolve_account_reference,
    resolve_local_account,
};
use crate::{AccountReference, actor_url, build_relationship_for_target};
use std::collections::HashSet;

const FAMILIAR_FOLLOWERS_LIMIT: usize = 3;

#[derive(Debug, Default, serde::Deserialize)]
struct MutesQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    max_id: Option<String>,
    #[serde(rename = "since_id")]
    since_id: Option<String>,
    #[serde(rename = "min_id")]
    min_id: Option<String>,
}

pub(crate) async fn account_relationships(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let db = ctx.d1(&config.database_binding)?;
    let viewer = resolve_local_account(&db, &user).await?;
    let mut relationships = Vec::new();

    for account_id in parse_relationship_query_ids(&req)? {
        match resolve_account_reference(&db, &account_id).await? {
            Some(AccountReference::Local(target)) => {
                relationships.push(
                    build_relationship_for_target(
                        &db,
                        &config,
                        &viewer,
                        &target.id,
                        &actor_url(&config, &target.username),
                    )
                    .await?,
                );
            }
            Some(AccountReference::Remote(actor)) => {
                relationships.push(
                    build_relationship_for_target(
                        &db,
                        &config,
                        &viewer,
                        &remote_account_rest_id(&actor.actor_uri),
                        &actor.actor_uri,
                    )
                    .await?,
                );
            }
            None => {}
        }
    }

    Response::from_json(&relationships)
}

fn build_familiar_followers_entry(
    account_id: &str,
    accounts: Vec<MastodonAccountResponse>,
) -> serde_json::Value {
    serde_json::json!({
        "id": account_id,
        "accounts": accounts,
    })
}

pub(crate) async fn familiar_followers_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let mut response = Vec::new();
    for requested_account_id in parse_relationship_query_ids(&req)? {
        let mut accounts = Vec::new();
        let mut seen_ids = HashSet::new();

        match resolve_account_reference(&db, &requested_account_id).await? {
            Some(AccountReference::Local(target)) => {
                for account in list_familiar_local_accounts_for_local_target(
                    &db,
                    &viewer.id,
                    &target.id,
                    FAMILIAR_FOLLOWERS_LIMIT as u32,
                )
                .await?
                {
                    let stats = load_account_stats(&db, &account.id).await?;
                    let response_account =
                        MastodonAccountResponse::from_account_with_stats(&account, &config, &stats);
                    if seen_ids.insert(response_account.id.clone()) {
                        accounts.push(response_account);
                    }
                    if accounts.len() >= FAMILIAR_FOLLOWERS_LIMIT {
                        break;
                    }
                }
                if accounts.len() < FAMILIAR_FOLLOWERS_LIMIT {
                    for actor in list_familiar_remote_actors_for_local_target(
                        &db,
                        &viewer.id,
                        &target.id,
                        (FAMILIAR_FOLLOWERS_LIMIT - accounts.len()) as u32,
                    )
                    .await?
                    {
                        let response_account = MastodonAccountResponse::from_remote_actor(&actor);
                        if seen_ids.insert(response_account.id.clone()) {
                            accounts.push(response_account);
                        }
                        if accounts.len() >= FAMILIAR_FOLLOWERS_LIMIT {
                            break;
                        }
                    }
                }
            }
            Some(AccountReference::Remote(actor)) => {
                for account in list_familiar_local_accounts_for_remote_target(
                    &db,
                    &viewer.id,
                    &actor.actor_uri,
                    FAMILIAR_FOLLOWERS_LIMIT as u32,
                )
                .await?
                {
                    let stats = load_account_stats(&db, &account.id).await?;
                    let response_account =
                        MastodonAccountResponse::from_account_with_stats(&account, &config, &stats);
                    if seen_ids.insert(response_account.id.clone()) {
                        accounts.push(response_account);
                    }
                    if accounts.len() >= FAMILIAR_FOLLOWERS_LIMIT {
                        break;
                    }
                }
            }
            None => {}
        }

        response.push(build_familiar_followers_entry(
            &requested_account_id,
            accounts,
        ));
    }

    Response::from_json(&response)
}

pub(crate) fn parse_relationship_query_ids(req: &Request) -> Result<Vec<String>> {
    let url = req.url()?;
    let mut ids = Vec::new();

    for (key, value) in url.query_pairs() {
        if key == "id[]" || key == "id" {
            let value = value.trim().to_owned();
            if !value.is_empty() {
                ids.push(value);
            }
        }
    }

    Ok(ids)
}

pub(crate) async fn mutes_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: MutesQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    let min_id = parse_internal_pagination_id(query.min_id.as_deref(), "min_id")?;
    let since_id = since_id.or(min_id);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let mutes = list_mutes_for_account(&db, &viewer.id, limit, max_id, since_id).await?;
    let mut response = Vec::new();
    for mute in &mutes {
        if let Some(target_account_id) = mute.target_account_id.as_deref()
            && let Some(account) = find_account_by_id(&db, target_account_id).await?
        {
            response.push(MastodonAccountResponse::from_account(&account, &config));
            continue;
        }

        if let Some(actor) = find_remote_actor_by_actor_uri(&db, &mute.target_actor_uri).await? {
            response.push(MastodonAccountResponse::from_remote_actor(&actor));
        }
    }

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        &req,
        limit,
        mutes.first().map(|mute| mute.cursor_id),
        mutes.last().map(|mute| mute.cursor_id),
        mutes.len() as u32 >= limit,
        max_id.is_some() || since_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }

    builder.from_json(&response)
}
