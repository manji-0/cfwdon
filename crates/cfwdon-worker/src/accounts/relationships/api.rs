use super::collection_entries::{
    local_account_follower_entries, local_account_following_entries, remote_actor_follower_entries,
    remote_actor_following_entries,
};
use super::collections::{
    AccountCollectionPage, AccountCollectionQuery, finalize_collection_response,
};
use super::query::parse_relationship_query_ids;
use super::resolution::resolve_requested_account_reference;
use crate::auth::find_authenticated_local_account;
use crate::instance::{actor_url, remote_account_rest_id};
use crate::relationships::build_relationship_for_target;
use crate::remote::{AccountReference, resolve_account_reference};
use crate::runtime_config::load_config;
use worker::{Request, Response, Result, RouteContext};

#[derive(Clone, Copy)]
enum AccountFollowCollectionKind {
    Followers,
    Following,
}

pub(crate) async fn account_relationships(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Auth0 authentication required", 401),
    };

    let relationships =
        futures_util::future::try_join_all(parse_relationship_query_ids(&req)?.into_iter().map(
            |account_id| {
                let db = &db;
                let config = &config;
                let viewer = &viewer;
                async move {
                    match resolve_requested_account_reference(db, config, &account_id).await? {
                        Some(AccountReference::Local(target)) => Ok::<_, worker::Error>(Some(
                            build_relationship_for_target(
                                db,
                                config,
                                viewer,
                                &target.id,
                                &actor_url(config, &target.username),
                            )
                            .await?,
                        )),
                        Some(AccountReference::Remote(actor)) => Ok::<_, worker::Error>(Some(
                            build_relationship_for_target(
                                db,
                                config,
                                viewer,
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

async fn account_follow_collection_response(
    req: Request,
    ctx: RouteContext<()>,
    kind: AccountFollowCollectionKind,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let AccountCollectionPage {
        limit,
        max_id,
        since_id,
    } = AccountCollectionPage::from_query(&query, 40, 80)?;
    let account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing account id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;

    let entries = match resolve_requested_account_reference(&db, &config, &account_id).await? {
        Some(AccountReference::Local(account)) => match kind {
            AccountFollowCollectionKind::Followers => {
                local_account_follower_entries(&db, &config, &account.id).await?
            }
            AccountFollowCollectionKind::Following => {
                local_account_following_entries(&db, &config, &account.id).await?
            }
        },
        Some(AccountReference::Remote(actor)) => match kind {
            AccountFollowCollectionKind::Followers => {
                remote_actor_follower_entries(
                    &db,
                    &config,
                    &actor.actor_uri,
                    limit.saturating_add(1),
                    max_id,
                    since_id,
                )
                .await?
            }
            AccountFollowCollectionKind::Following => {
                remote_actor_following_entries(
                    &db,
                    &config,
                    &actor.actor_uri,
                    limit.saturating_add(1),
                    max_id,
                    since_id,
                )
                .await?
            }
        },
        None => return Response::error("account not found", 404),
    };

    finalize_collection_response(&req, limit, max_id, since_id, entries)
}

pub(crate) async fn account_followers_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    account_follow_collection_response(req, ctx, AccountFollowCollectionKind::Followers).await
}

pub(crate) async fn account_following_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    account_follow_collection_response(req, ctx, AccountFollowCollectionKind::Following).await
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
        None => return Response::error("Auth0 authentication required", 401),
    }

    if resolve_account_reference(&db, &account_id).await?.is_none() {
        return Response::error("account not found", 404);
    }

    Response::from_json(&Vec::<serde_json::Value>::new())
}
