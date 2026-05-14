use super::query::parse_relationship_query_ids;
use crate::AccountReference;
use crate::{
    MastodonAccountResponse, Request, Response, Result, RouteContext,
    find_authenticated_local_account, list_familiar_local_accounts_for_local_target,
    list_familiar_local_accounts_for_remote_target, list_familiar_remote_actors_for_local_target,
    load_account_stats, load_config, resolve_account_reference,
};
use std::collections::HashSet;

const FAMILIAR_FOLLOWERS_LIMIT: usize = 3;

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
