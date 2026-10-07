use crate::accounts::{
    DirectoryOrder, list_discoverable_accounts_with_sort_key, load_account_stats_map,
};
use crate::auth::find_authenticated_local_account;
use crate::db_session::bind_request_d1;
use crate::db_utils::d1_results;
use crate::identity::actor_url;
use crate::responses::MastodonAccountResponse;
use crate::runtime_config::load_config;
use crate::store::relationship::list_active_muted_actor_uris_for_account;
use crate::tracked_d1::D1Database;
use serde::Deserialize;
use std::collections::HashSet;
use worker::d1::D1Type;
use worker::{Request, Response, Result, RouteContext};

/// Actors the account follows (in any state) or blocks, which suggestions skip.
async fn list_followed_or_blocked_actor_uris(
    db: &D1Database,
    account_id: &str,
) -> Result<HashSet<String>> {
    #[derive(Deserialize)]
    struct ActorUriRow {
        target_actor_uri: String,
    }
    let result = db
        .prepare(
            "SELECT target_actor_uri
             FROM follows
             WHERE follower_account_id = ?1
               AND target_actor_uri IS NOT NULL
             UNION
             SELECT target_actor_uri
             FROM blocks
             WHERE blocker_account_id = ?1",
        )
        .bind_refs(&D1Type::Text(account_id))?
        .all()
        .await?;
    Ok(d1_results::<ActorUriRow>(&result)?
        .into_iter()
        .map(|row| row.target_actor_uri)
        .collect())
}

#[derive(Debug, Default, Deserialize)]
struct SuggestionsQuery {
    limit: Option<u32>,
}

async fn suggested_accounts(
    req: &Request,
    ctx: &RouteContext<()>,
) -> Result<Option<Vec<MastodonAccountResponse>>> {
    let config = load_config(ctx);
    let query: SuggestionsQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let db = bind_request_d1(ctx, &config)?;
    let Some(viewer) = find_authenticated_local_account(req, &db, &config).await? else {
        return Ok(None);
    };

    let (candidates, excluded_actor_uris, muted_actor_uris) = futures_util::try_join!(
        list_discoverable_accounts_with_sort_key(&db, 200, 0, DirectoryOrder::Active),
        list_followed_or_blocked_actor_uris(&db, viewer.id()),
        list_active_muted_actor_uris_for_account(&db, viewer.id()),
    )?;
    let accounts = candidates
        .into_iter()
        .map(|(account, _sort_key)| account)
        .filter(|account| {
            let actor_uri = actor_url(&config, account.username());
            account.id() != viewer.id()
                && !excluded_actor_uris.contains(&actor_uri)
                && !muted_actor_uris.contains(&actor_uri)
        })
        .take(limit as usize)
        .collect::<Vec<_>>();
    let account_ids = accounts
        .iter()
        .map(|account| account.id().to_owned())
        .collect::<Vec<_>>();
    let stats_by_id = load_account_stats_map(&db, &account_ids).await?;
    let default_stats = Default::default();
    let suggestions = accounts
        .iter()
        .map(|account| {
            MastodonAccountResponse::from_account_with_stats(
                account,
                &config,
                stats_by_id.get(account.id()).unwrap_or(&default_stats),
            )
        })
        .collect::<Vec<_>>();

    Ok(Some(suggestions))
}

pub(crate) async fn suggestions_v1_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match suggested_accounts(&req, &ctx).await {
        Ok(Some(accounts)) => Response::from_json(&accounts),
        Ok(None) => Response::error("Auth0 authentication required", 401),
        Err(error) => Err(error),
    }
}

pub(crate) async fn delete_suggestion_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    if find_authenticated_local_account(&req, &db, &config)
        .await?
        .is_none()
    {
        return Response::error("Auth0 authentication required", 401);
    }

    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn suggestions_v2_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match suggested_accounts(&req, &ctx).await {
        Ok(Some(accounts)) => {
            let suggestions = accounts
                .into_iter()
                .map(|account| {
                    serde_json::json!({
                        "source": "global",
                        "account": account,
                    })
                })
                .collect::<Vec<_>>();
            Response::from_json(&suggestions)
        }
        Ok(None) => Response::error("Auth0 authentication required", 401),
        Err(error) => Err(error),
    }
}
