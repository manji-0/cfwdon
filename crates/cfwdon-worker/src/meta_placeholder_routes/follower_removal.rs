use crate::activitypub::build_reject_follow_activity;
use crate::auth::find_authenticated_local_account;
use crate::db_session::bind_request_d1;
use crate::delivery::queue_remote_actor_activity_required;
use crate::follow_requests::{
    delete_remote_follow_request_by_actor, find_pending_remote_follow_request_by_actor,
};
use crate::identity::{actor_url, remote_account_rest_id};
use crate::inbox::{delete_follower_by_actor, find_follower_follow_activity_id};
use crate::relationships::build_relationship_for_target;
use crate::remote::{AccountReference, resolve_account_reference};
use crate::runtime_config::load_config;
use crate::store::relationship::delete_follow_by_target;
use worker::{Request, Response, Result, RouteContext};

pub(crate) async fn remove_from_followers_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing account id route parameter".to_owned()))?;

    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            let target_actor_uri = actor_url(&config, target.username());
            delete_follow_by_target(&db, target.id(), &actor_url(&config, viewer.username()))
                .await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &viewer,
                target.id(),
                &target_actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            let accepted_follow_activity_id = find_follower_follow_activity_id(
                &db,
                viewer.id(),
                &actor.actor_uri,
                &actor.actor_uri,
            )
            .await?;
            if let Some(request) =
                find_pending_remote_follow_request_by_actor(&db, viewer.id(), &actor.actor_uri)
                    .await?
            {
                delete_remote_follow_request_by_actor(
                    &db,
                    viewer.id(),
                    &actor.actor_uri,
                    &actor.actor_uri,
                )
                .await?;
                if let Some(follow_activity_id) = request.follow_activity_id.as_deref() {
                    let payload = build_reject_follow_activity(
                        &config,
                        &viewer,
                        follow_activity_id,
                        &actor.actor_uri,
                    )?;
                    let _ = queue_remote_actor_activity_required(
                        &db,
                        viewer.id(),
                        &actor.actor_uri,
                        &payload,
                    )
                    .await;
                }
            }
            if let Some(follow_activity_id) = accepted_follow_activity_id.as_deref() {
                let payload = build_reject_follow_activity(
                    &config,
                    &viewer,
                    follow_activity_id,
                    &actor.actor_uri,
                )?;
                let _ = queue_remote_actor_activity_required(
                    &db,
                    viewer.id(),
                    &actor.actor_uri,
                    &payload,
                )
                .await;
            }
            delete_follower_by_actor(&db, viewer.id(), &actor.actor_uri, &actor.actor_uri).await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &viewer,
                &remote_account_rest_id(&actor.actor_uri),
                &actor.actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        None => Response::error("account not found", 404),
    }
}
