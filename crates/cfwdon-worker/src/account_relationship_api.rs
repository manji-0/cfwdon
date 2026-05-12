use super::{
    MastodonAccountResponse, Request, Response, Result, RouteContext,
    build_internal_cursor_link_header, fetch_remote_actor_profile, find_account_by_id,
    find_account_by_username, find_authenticated_local_account, find_remote_actor_by_actor_uri,
    find_remote_actor_by_username_domain, list_blocks_for_account,
    list_familiar_local_accounts_for_local_target, list_familiar_local_accounts_for_remote_target,
    list_familiar_remote_actors_for_local_target, list_local_followers_for_account,
    list_local_followers_for_remote_actor, list_local_following_for_account,
    list_local_following_for_remote_actor, list_mutes_for_account,
    list_remote_followers_for_account, list_remote_following_for_account, load_account_stats,
    load_config, parse_internal_pagination_id, parse_lookup_handle, remote_account_rest_id,
    resolve_account_reference, upsert_remote_actor,
};
use crate::{AccountReference, actor_url, build_relationship_for_target};
use std::collections::HashSet;

const FAMILIAR_FOLLOWERS_LIMIT: usize = 3;

#[derive(Debug, Default, serde::Deserialize)]
struct AccountCollectionQuery {
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
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let relationships =
        futures_util::future::try_join_all(parse_relationship_query_ids(&req)?.into_iter().map(
            |account_id| {
                let db = &db;
                let config = &config;
                let viewer = &viewer;
                async move {
                    match resolve_requested_account_reference(&db, &config, &account_id).await? {
                        Some(AccountReference::Local(target)) => Ok::<_, worker::Error>(Some(
                            build_relationship_for_target(
                                &db,
                                &config,
                                &viewer,
                                &target.id,
                                &actor_url(&config, &target.username),
                            )
                            .await?,
                        )),
                        Some(AccountReference::Remote(actor)) => Ok::<_, worker::Error>(Some(
                            build_relationship_for_target(
                                &db,
                                &config,
                                &viewer,
                                &remote_account_rest_id(&actor.actor_uri),
                                &actor.actor_uri,
                            )
                            .await?,
                        )),
                        None => Ok::<_, worker::Error>(None),
                    }
                }
            },
        ))
        .await?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

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

#[derive(Debug)]
struct CollectionAccountEntry {
    cursor_id: i64,
    created_at: String,
    account: MastodonAccountResponse,
}

async fn resolve_requested_account_reference(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    account_id: &str,
) -> Result<Option<AccountReference>> {
    if let Some(reference) = resolve_account_reference(db, account_id).await? {
        return Ok(Some(reference));
    }

    let handle = match parse_lookup_handle(account_id, config) {
        Ok(handle) => handle,
        Err(_) => return Ok(None),
    };

    if handle.is_local_to(&config.instance_domain) {
        return Ok(find_account_by_username(db, &handle.username)
            .await?
            .map(AccountReference::Local));
    }

    let Some(domain) = handle.domain.as_deref() else {
        return Ok(None);
    };
    Ok(
        find_remote_actor_by_username_domain(db, &handle.username, domain)
            .await?
            .map(AccountReference::Remote),
    )
}

fn finalize_collection_response(
    req: &Request,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
    mut entries: Vec<CollectionAccountEntry>,
) -> Result<Response> {
    entries.retain(|entry| max_id.is_none_or(|value| entry.cursor_id < value));
    entries.retain(|entry| since_id.is_none_or(|value| entry.cursor_id > value));
    entries.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.cursor_id.cmp(&left.cursor_id))
    });
    if entries.len() > limit as usize {
        entries.truncate(limit as usize);
    }

    let first_id = entries.first().map(|entry| entry.cursor_id);
    let last_id = entries.last().map(|entry| entry.cursor_id);
    let response = entries
        .into_iter()
        .map(|entry| entry.account)
        .collect::<Vec<_>>();

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        req,
        limit,
        first_id,
        last_id,
        response.len() as u32 >= limit,
        max_id.is_some() || since_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }

    builder.from_json(&response)
}

async fn remote_follow_account_response(
    db: &worker::D1Database,
    actor_uri: &str,
) -> Result<Option<MastodonAccountResponse>> {
    match fetch_remote_actor_profile(actor_uri).await {
        Ok(profile) => {
            upsert_remote_actor(db, &profile).await?;
            match find_remote_actor_by_actor_uri(db, &profile.actor_uri).await? {
                Some(actor) => Ok(Some(MastodonAccountResponse::from_remote_actor(&actor))),
                None => Ok(Some(MastodonAccountResponse::from_remote_actor_profile(
                    &profile,
                ))),
            }
        }
        Err(_) => Ok(find_remote_actor_by_actor_uri(db, actor_uri)
            .await?
            .map(|actor| MastodonAccountResponse::from_remote_actor(&actor))),
    }
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

pub(crate) async fn account_followers_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    let min_id = parse_internal_pagination_id(query.min_id.as_deref(), "min_id")?;
    let since_id = since_id.or(min_id);
    let account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing account id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;

    let mut entries = Vec::new();
    match resolve_requested_account_reference(&db, &config, &account_id).await? {
        Some(AccountReference::Local(account)) => {
            for follower in list_local_followers_for_account(&db, &account.id).await? {
                if let Some(author) = find_account_by_id(&db, &follower.account_id).await? {
                    let stats = load_account_stats(&db, &author.id).await?;
                    entries.push(CollectionAccountEntry {
                        cursor_id: follower.cursor_id,
                        created_at: follower.created_at,
                        account: MastodonAccountResponse::from_account_with_stats(
                            &author, &config, &stats,
                        ),
                    });
                }
            }
            for follower in list_remote_followers_for_account(&db, &account.id).await? {
                if let Some(account) =
                    remote_follow_account_response(&db, &follower.actor_uri).await?
                {
                    entries.push(CollectionAccountEntry {
                        cursor_id: follower.cursor_id,
                        created_at: follower.created_at,
                        account,
                    });
                }
            }
        }
        Some(AccountReference::Remote(actor)) => {
            for follower in list_local_followers_for_remote_actor(&db, &actor.actor_uri).await? {
                if let Some(account) = find_account_by_id(&db, &follower.account_id).await? {
                    let stats = load_account_stats(&db, &account.id).await?;
                    entries.push(CollectionAccountEntry {
                        cursor_id: follower.cursor_id,
                        created_at: follower.created_at,
                        account: MastodonAccountResponse::from_account_with_stats(
                            &account, &config, &stats,
                        ),
                    });
                }
            }
        }
        None => return Response::error("account not found", 404),
    }

    finalize_collection_response(&req, limit, max_id, since_id, entries)
}

pub(crate) async fn account_following_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    let min_id = parse_internal_pagination_id(query.min_id.as_deref(), "min_id")?;
    let since_id = since_id.or(min_id);
    let account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing account id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;

    let mut entries = Vec::new();
    match resolve_requested_account_reference(&db, &config, &account_id).await? {
        Some(AccountReference::Local(account)) => {
            for followed in list_local_following_for_account(&db, &account.id).await? {
                if let Some(target) = find_account_by_id(&db, &followed.account_id).await? {
                    let stats = load_account_stats(&db, &target.id).await?;
                    entries.push(CollectionAccountEntry {
                        cursor_id: followed.cursor_id,
                        created_at: followed.created_at,
                        account: MastodonAccountResponse::from_account_with_stats(
                            &target, &config, &stats,
                        ),
                    });
                }
            }
            for followed in list_remote_following_for_account(&db, &account.id).await? {
                if let Some(account) =
                    remote_follow_account_response(&db, &followed.actor_uri).await?
                {
                    entries.push(CollectionAccountEntry {
                        cursor_id: followed.cursor_id,
                        created_at: followed.created_at,
                        account,
                    });
                }
            }
        }
        Some(AccountReference::Remote(actor)) => {
            for followed in list_local_following_for_remote_actor(&db, &actor.actor_uri).await? {
                if let Some(account) = find_account_by_id(&db, &followed.account_id).await? {
                    let stats = load_account_stats(&db, &account.id).await?;
                    entries.push(CollectionAccountEntry {
                        cursor_id: followed.cursor_id,
                        created_at: followed.created_at,
                        account: MastodonAccountResponse::from_account_with_stats(
                            &account, &config, &stats,
                        ),
                    });
                }
            }
        }
        None => return Response::error("account not found", 404),
    }

    finalize_collection_response(&req, limit, max_id, since_id, entries)
}

pub(crate) async fn identity_proofs_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing account id route parameter".to_owned()))?;

    match find_authenticated_local_account(&req, &db, &config).await? {
        Some(_) => {}
        None => return Response::error("Cloudflare Access authentication required", 401),
    }

    if resolve_account_reference(&db, &account_id).await?.is_none() {
        return Response::error("account not found", 404);
    }

    Response::from_json(&Vec::<serde_json::Value>::new())
}

pub(crate) async fn blocks_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
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

    let blocks = list_blocks_for_account(&db, &viewer.id, limit, max_id, since_id).await?;
    let mut response = Vec::new();
    for block in &blocks {
        if let Some(target_account_id) = block.target_account_id.as_deref()
            && let Some(account) = find_account_by_id(&db, target_account_id).await?
        {
            response.push(MastodonAccountResponse::from_account(&account, &config));
            continue;
        }

        if let Some(actor) = find_remote_actor_by_actor_uri(&db, &block.target_actor_uri).await? {
            response.push(MastodonAccountResponse::from_remote_actor(&actor));
        }
    }

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        &req,
        limit,
        blocks.first().map(|block| block.cursor_id),
        blocks.last().map(|block| block.cursor_id),
        blocks.len() as u32 >= limit,
        max_id.is_some() || since_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }

    builder.from_json(&response)
}

pub(crate) async fn mutes_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
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
