use super::{
    AppConfig, D1Database, LocalAccount, Request, Response, Result, RouteContext,
    begin_inbox_activity_processing, handle_inbox_accept, handle_inbox_announce,
    handle_inbox_create, handle_inbox_delete, handle_inbox_follow, handle_inbox_like,
    handle_inbox_reject, handle_inbox_undo, handle_inbox_update, inbox_activity_id, load_config,
    mark_inbox_activity_processed, release_inbox_activity_processing, resolve_inbox_target_account,
    verify_incoming_activitypub_request,
};
use worker::Error;

const ACTIVITYPUB_UNAUTHORIZED_PREFIX: &str = "activitypub unauthorized:";

pub(crate) async fn shared_inbox_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let body = req.bytes().await?;
    let activity: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| Error::RustError(format!("invalid activitypub payload: {error}")))?;

    if activity.get("type").and_then(serde_json::Value::as_str) == Some("Delete") {
        return handle_inbox_request(&req, &db, &config, None, &body, &activity).await;
    }

    let Some(account) = resolve_inbox_target_account(&db, &config, None, &activity).await? else {
        return Ok(Response::empty()?.with_status(202));
    };
    handle_inbox_request(&req, &db, &config, Some(&account), &body, &activity).await
}

pub(crate) async fn inbox_response(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;

    let body = req.bytes().await?;
    let activity: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| Error::RustError(format!("invalid activitypub payload: {error}")))?;
    let db = ctx.d1(&config.database_binding)?;
    let Some(account) =
        resolve_inbox_target_account(&db, &config, Some(username.as_str()), &activity).await?
    else {
        return Response::error("actor not found", 404);
    };
    handle_inbox_request(&req, &db, &config, Some(&account), &body, &activity).await
}

pub(crate) async fn handle_inbox_request(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
    account: Option<&LocalAccount>,
    body: &[u8],
    activity: &serde_json::Value,
) -> Result<Response> {
    let remote_actor = match verify_incoming_activitypub_request(req, db, body, activity).await {
        Ok(remote_actor) => remote_actor,
        Err(_) => return Response::error("invalid activitypub signature", 401),
    };
    let activity_id = inbox_activity_id(activity);
    if let Some(activity_id) = activity_id.as_deref()
        && !begin_inbox_activity_processing(
            db,
            &remote_actor.actor_uri,
            activity_id,
            activity
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
        )
        .await?
    {
        return Ok(Response::empty()?.with_status(202));
    }

    let result = match activity.get("type").and_then(serde_json::Value::as_str) {
        Some("Follow") => {
            if let Some(account) = account {
                handle_inbox_follow(db, config, account, activity, &remote_actor).await
            } else {
                Ok(())
            }
        }
        Some("Undo") => {
            if let Some(account) = account {
                handle_inbox_undo(db, account, activity, &remote_actor, config).await
            } else {
                Ok(())
            }
        }
        Some("Accept") => handle_inbox_accept(db, activity, &remote_actor).await,
        Some("Reject") => handle_inbox_reject(db, activity, &remote_actor).await,
        Some("Like") => {
            if let Some(account) = account {
                handle_inbox_like(db, activity, &remote_actor, account, config).await
            } else {
                Ok(())
            }
        }
        Some("Create") => {
            if let Some(account) = account {
                handle_inbox_create(db, activity, &remote_actor, account, config).await
            } else {
                Ok(())
            }
        }
        Some("Announce") => {
            if let Some(account) = account {
                handle_inbox_announce(db, activity, &remote_actor, account, config).await
            } else {
                Ok(())
            }
        }
        Some("Update") => {
            if let Some(account) = account {
                handle_inbox_update(db, activity, &remote_actor, account, config).await
            } else {
                Ok(())
            }
        }
        Some("Delete") => handle_inbox_delete(db, activity, &remote_actor).await,
        _ => Ok(()),
    };

    if let Some(activity_id) = activity_id.as_deref() {
        match &result {
            Ok(()) => {
                mark_inbox_activity_processed(db, &remote_actor.actor_uri, activity_id).await?
            }
            Err(_) => {
                release_inbox_activity_processing(db, &remote_actor.actor_uri, activity_id).await?
            }
        }
    }

    match result {
        Ok(()) => Ok(Response::empty()?.with_status(202)),
        Err(Error::RustError(message)) if message.starts_with(ACTIVITYPUB_UNAUTHORIZED_PREFIX) => {
            Response::error("unauthorized activitypub object attribution", 401)
        }
        Err(error) => Err(error),
    }
}
