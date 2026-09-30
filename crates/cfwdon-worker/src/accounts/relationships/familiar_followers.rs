use super::query::parse_relationship_query_ids;
use crate::accounts::build_local_account_response;
use crate::auth::find_authenticated_local_account;
use crate::db_session::bind_request_d1;
use crate::relationship::{
    list_familiar_local_accounts_for_local_target, list_familiar_local_accounts_for_remote_target,
    list_familiar_remote_actors_for_local_target,
};
use crate::remote::{AccountReference, RemoteActorRow, resolve_account_reference};
use crate::responses::MastodonAccountResponse;
use crate::runtime_config::load_config;
use crate::tracked_d1::D1Database;
use cfwdon_domain::LocalAccount;
use std::collections::HashSet;
use worker::{Request, Response, Result, RouteContext};

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

fn push_unique_familiar_account(
    accounts: &mut Vec<MastodonAccountResponse>,
    seen_ids: &mut HashSet<String>,
    response_account: MastodonAccountResponse,
) {
    if seen_ids.insert(response_account.id.clone()) {
        accounts.push(response_account);
    }
}

async fn append_familiar_local_accounts(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    accounts: &mut Vec<MastodonAccountResponse>,
    seen_ids: &mut HashSet<String>,
    local_accounts: Vec<LocalAccount>,
) -> Result<()> {
    for account in local_accounts {
        push_unique_familiar_account(
            accounts,
            seen_ids,
            build_local_account_response(db, config, &account).await?,
        );
        if accounts.len() >= FAMILIAR_FOLLOWERS_LIMIT {
            break;
        }
    }
    Ok(())
}

fn append_familiar_remote_actors(
    accounts: &mut Vec<MastodonAccountResponse>,
    seen_ids: &mut HashSet<String>,
    remote_actors: Vec<RemoteActorRow>,
) {
    for actor in remote_actors {
        push_unique_familiar_account(
            accounts,
            seen_ids,
            MastodonAccountResponse::from_remote_actor(&actor),
        );
        if accounts.len() >= FAMILIAR_FOLLOWERS_LIMIT {
            break;
        }
    }
}

pub(crate) async fn familiar_followers_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };

    let mut response = Vec::new();
    for requested_account_id in parse_relationship_query_ids(&req)? {
        let mut accounts = Vec::new();
        let mut seen_ids = HashSet::new();

        match resolve_account_reference(&db, &requested_account_id).await? {
            Some(AccountReference::Local(target)) => {
                append_familiar_local_accounts(
                    &db,
                    &config,
                    &mut accounts,
                    &mut seen_ids,
                    list_familiar_local_accounts_for_local_target(
                        &db,
                        viewer.id(),
                        target.id(),
                        FAMILIAR_FOLLOWERS_LIMIT as u32,
                    )
                    .await?,
                )
                .await?;
                if accounts.len() < FAMILIAR_FOLLOWERS_LIMIT {
                    let remaining = (FAMILIAR_FOLLOWERS_LIMIT - accounts.len()) as u32;
                    append_familiar_remote_actors(
                        &mut accounts,
                        &mut seen_ids,
                        list_familiar_remote_actors_for_local_target(
                            &db,
                            viewer.id(),
                            target.id(),
                            remaining,
                        )
                        .await?,
                    );
                }
            }
            Some(AccountReference::Remote(actor)) => {
                append_familiar_local_accounts(
                    &db,
                    &config,
                    &mut accounts,
                    &mut seen_ids,
                    list_familiar_local_accounts_for_remote_target(
                        &db,
                        viewer.id(),
                        &actor.actor_uri,
                        FAMILIAR_FOLLOWERS_LIMIT as u32,
                    )
                    .await?,
                )
                .await?;
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
