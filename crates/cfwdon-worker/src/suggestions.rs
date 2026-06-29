use crate::{
    DirectoryOrder, MastodonAccountResponse, Request, Response, Result, RouteContext, actor_url,
    find_follow_by_target, is_blocking_actor, is_muted_actor,
    list_discoverable_accounts_with_sort_key, load_account_stats, load_config,
    require_authenticated_local_account,
};
use serde::Deserialize;

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
    let db = ctx.d1(&config.database_binding)?;
    let Some(viewer) = require_authenticated_local_account(req, &db, &config).await? else {
        return Ok(None);
    };

    let mut suggestions = Vec::new();
    for (account, _sort_key) in
        list_discoverable_accounts_with_sort_key(&db, 200, 0, DirectoryOrder::Active).await?
    {
        if account.id() == viewer.id() {
            continue;
        }
        let target_actor_uri = actor_url(&config, account.username());
        if find_follow_by_target(&db, viewer.id(), &target_actor_uri)
            .await?
            .is_some()
            || is_blocking_actor(&db, viewer.id(), &target_actor_uri).await?
            || is_muted_actor(&db, viewer.id(), &target_actor_uri).await?
        {
            continue;
        }
        let stats = load_account_stats(&db, account.id()).await?;
        suggestions.push(MastodonAccountResponse::from_account_with_stats(
            &account, &config, &stats,
        ));
        if suggestions.len() >= limit as usize {
            break;
        }
    }

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
    let db = ctx.d1(&config.database_binding)?;
    if require_authenticated_local_account(&req, &db, &config)
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
