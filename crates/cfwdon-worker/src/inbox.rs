use crate::D1Database;
#[allow(unused_imports)]
pub(crate) use crate::*;

mod activity_store;
mod actor_updates;
mod follow_handlers;
mod interactions;
mod poll_interactions;
mod status_handlers;
mod status_interactions;
mod target_resolution;
pub(crate) use activity_store::*;
pub(crate) use actor_updates::*;
pub(crate) use follow_handlers::*;
pub(crate) use interactions::*;
pub(crate) use poll_interactions::*;
pub(crate) use status_handlers::*;
pub(crate) use status_interactions::*;
pub(crate) use target_resolution::*;

use worker::{Env, Error};

const ACTIVITYPUB_UNAUTHORIZED_PREFIX: &str = "activitypub unauthorized:";

fn inbox_activity_type(activity: &serde_json::Value) -> &str {
    activitypub_primary_type(activity).unwrap_or_default()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SharedInboxPreprocessOutcome {
    Unauthorized,
    AcceptedNoTargets,
    Process,
}

/// Shared-inbox gate after signature verification and target resolution.
/// Signature verification must always run before consulting `has_local_targets`
/// so unsigned requests cannot probe account existence via status codes.
pub(crate) fn shared_inbox_preprocess_outcome(
    signature_ok: bool,
    has_local_targets: bool,
) -> SharedInboxPreprocessOutcome {
    if !signature_ok {
        SharedInboxPreprocessOutcome::Unauthorized
    } else if !has_local_targets {
        SharedInboxPreprocessOutcome::AcceptedNoTargets
    } else {
        SharedInboxPreprocessOutcome::Process
    }
}

async fn begin_inbox_activity_if_needed(
    db: &D1Database,
    remote_actor: &RemoteActorProfile,
    activity_id: &str,
    activity_type: &str,
) -> Result<bool> {
    begin_inbox_activity_processing(db, &remote_actor.actor_uri, activity_id, activity_type).await
}

async fn finish_inbox_activity_if_needed(
    db: &D1Database,
    remote_actor: &RemoteActorProfile,
    activity_id: &str,
    result: &Result<()>,
) -> Result<()> {
    match result {
        Ok(()) => mark_inbox_activity_processed(db, &remote_actor.actor_uri, activity_id).await?,
        Err(_) => {
            release_inbox_activity_processing(db, &remote_actor.actor_uri, activity_id).await?
        }
    }
    Ok(())
}

async fn dispatch_inbox_activity(
    db: &D1Database,
    config: &AppConfig,
    account: Option<&LocalAccount>,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    env: Option<&Env>,
) -> Result<()> {
    match inbox_activity_type(activity) {
        "Follow" => {
            if let Some(account) = account {
                handle_inbox_follow(db, config, account, activity, remote_actor, env).await
            } else {
                Ok(())
            }
        }
        "Undo" => {
            if let Some(account) = account {
                handle_inbox_undo(db, account, activity, remote_actor, config, env).await
            } else {
                Ok(())
            }
        }
        "Accept" => handle_inbox_accept(db, config, activity, remote_actor).await,
        "Reject" => handle_inbox_reject(db, activity, remote_actor).await,
        "Like" => {
            if let Some(account) = account {
                handle_inbox_like(db, activity, remote_actor, account, config, env).await
            } else {
                Ok(())
            }
        }
        "Create" => {
            if let Some(account) = account {
                handle_inbox_create(db, activity, remote_actor, account, config, env).await
            } else {
                Ok(())
            }
        }
        "Announce" => {
            if let Some(account) = account {
                handle_inbox_announce(db, activity, remote_actor, account, config, env).await
            } else {
                Ok(())
            }
        }
        "Add" => {
            if account.is_some() {
                handle_inbox_collection_add(db, config, activity, remote_actor).await
            } else {
                Ok(())
            }
        }
        "Remove" => {
            if account.is_some() {
                handle_inbox_collection_remove(db, activity, remote_actor).await
            } else {
                Ok(())
            }
        }
        "Update" => {
            if let Some(account) = account {
                handle_inbox_update(db, activity, remote_actor, account, config, env).await
            } else {
                Ok(())
            }
        }
        "Delete" => handle_inbox_delete(db, config, activity, remote_actor, env).await,
        _ => Ok(()),
    }
}

fn inbox_result_response(result: Result<()>) -> Result<Response> {
    match result {
        Ok(()) => Ok(Response::empty()?.with_status(202)),
        Err(Error::RustError(message)) if message.starts_with(ACTIVITYPUB_UNAUTHORIZED_PREFIX) => {
            Response::error("unauthorized activitypub object attribution", 401)
        }
        Err(error) => Err(error),
    }
}

pub(crate) async fn shared_inbox_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let body = req.bytes().await?;
    let activity: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| Error::RustError(format!("invalid activitypub payload: {error}")))?;

    if activitypub_has_type(&activity, "Delete") {
        return handle_inbox_request(&req, &db, &config, None, &body, &activity, Some(&ctx.env))
            .await;
    }

    let remote_actor = match verify_incoming_activitypub_request(&req, &db, &body, &activity).await
    {
        Ok(remote_actor) => remote_actor,
        Err(error) => {
            log_federation_event(
                "inbox_signature_failed",
                "unauthorized",
                format!(
                    "shared inbox signature verification failed: activity_type={} error={error}",
                    inbox_activity_type(&activity)
                ),
                serde_json::json!({
                    "inbox": "shared",
                    "activity_type": inbox_activity_type(&activity),
                    "error": error.to_string(),
                }),
            );
            return Response::error("invalid activitypub signature", 401);
        }
    };
    let accounts = resolve_shared_inbox_target_accounts(&db, &config, None, &activity).await?;
    match shared_inbox_preprocess_outcome(true, !accounts.is_empty()) {
        SharedInboxPreprocessOutcome::Unauthorized => {
            Response::error("invalid activitypub signature", 401)
        }
        SharedInboxPreprocessOutcome::AcceptedNoTargets => {
            log_federation_event(
                "inbox_accepted_no_targets",
                "skipped",
                format!(
                    "shared inbox accepted with no local targets: activity_type={} actor={}",
                    inbox_activity_type(&activity),
                    remote_actor.actor_uri
                ),
                serde_json::json!({
                    "inbox": "shared",
                    "activity_type": inbox_activity_type(&activity),
                    "actor_uri": remote_actor.actor_uri,
                }),
            );
            Ok(Response::empty()?.with_status(202))
        }
        SharedInboxPreprocessOutcome::Process => {
            process_verified_inbox_activity(
                &db,
                &config,
                &accounts,
                &body,
                &activity,
                remote_actor,
                Some(&ctx.env),
            )
            .await
        }
    }
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
    let db = crate::bind_request_d1(&ctx, &config)?;
    let accounts =
        resolve_shared_inbox_target_accounts(&db, &config, Some(username.as_str()), &activity)
            .await?;
    let Some(account) = accounts.into_iter().next() else {
        return Response::error("actor not found", 404);
    };
    handle_inbox_request(
        &req,
        &db,
        &config,
        Some(&account),
        &body,
        &activity,
        Some(&ctx.env),
    )
    .await
}

pub(crate) async fn handle_inbox_request(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
    account: Option<&LocalAccount>,
    body: &[u8],
    activity: &serde_json::Value,
    env: Option<&Env>,
) -> Result<Response> {
    let accounts = account
        .map(|account| vec![account.clone()])
        .unwrap_or_default();
    handle_inbox_request_for_accounts(req, db, config, &accounts, body, activity, env).await
}

pub(crate) async fn handle_inbox_request_for_accounts(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
    accounts: &[LocalAccount],
    body: &[u8],
    activity: &serde_json::Value,
    env: Option<&Env>,
) -> Result<Response> {
    let remote_actor = match verify_incoming_activitypub_request(req, db, body, activity).await {
        Ok(remote_actor) => remote_actor,
        Err(error) => {
            log_federation_event(
                "inbox_signature_failed",
                "unauthorized",
                format!(
                    "inbox signature verification failed: activity_type={} error={error}",
                    inbox_activity_type(activity)
                ),
                serde_json::json!({
                    "inbox": "personal",
                    "activity_type": inbox_activity_type(activity),
                    "error": error.to_string(),
                }),
            );
            return Response::error("invalid activitypub signature", 401);
        }
    };
    process_verified_inbox_activity(db, config, accounts, body, activity, remote_actor, env).await
}

async fn process_verified_inbox_activity(
    db: &D1Database,
    config: &AppConfig,
    accounts: &[LocalAccount],
    body: &[u8],
    activity: &serde_json::Value,
    remote_actor: RemoteActorProfile,
    env: Option<&Env>,
) -> Result<Response> {
    let activity_type = inbox_activity_type(activity).to_owned();
    let activity_id = inbox_activity_dedupe_id(activity, &remote_actor.actor_uri, body).await?;
    let remote_actor = &remote_actor;
    let activity_id = activity_id.as_str();
    let activity_type = activity_type.as_str();
    if !begin_inbox_activity_if_needed(db, remote_actor, activity_id, activity_type).await? {
        log_federation_event(
            "inbox_replay_skipped",
            "replay",
            format!(
                "inbox replay skipped: activity_type={activity_type} actor={} activity_id={activity_id}",
                remote_actor.actor_uri
            ),
            serde_json::json!({
                "activity_type": activity_type,
                "activity_id": activity_id,
                "actor_uri": remote_actor.actor_uri,
                "target_count": accounts.len(),
            }),
        );
        return Ok(Response::empty()?.with_status(202));
    }

    let result = if accounts.is_empty() {
        dispatch_inbox_activity(db, config, None, activity, remote_actor, env).await
    } else {
        let mut outcome = Ok(());
        for account in accounts {
            match dispatch_inbox_activity(db, config, Some(account), activity, remote_actor, env)
                .await
            {
                Ok(()) => {}
                Err(error) => {
                    outcome = Err(error);
                    break;
                }
            }
        }
        outcome
    };
    finish_inbox_activity_if_needed(db, remote_actor, activity_id, &result).await?;
    match &result {
        Ok(()) => log_federation_event(
            "inbox_processed",
            "ok",
            format!(
                "inbox processed: activity_type={activity_type} actor={} activity_id={activity_id} targets={}",
                remote_actor.actor_uri,
                accounts.len()
            ),
            serde_json::json!({
                "activity_type": activity_type,
                "activity_id": activity_id,
                "actor_uri": remote_actor.actor_uri,
                "target_count": accounts.len(),
                "target_account_ids": accounts.iter().map(LocalAccount::id).collect::<Vec<_>>(),
            }),
        ),
        Err(error) => log_federation_event(
            "inbox_processing_failed",
            "failed",
            format!(
                "inbox processing failed: activity_type={activity_type} actor={} activity_id={activity_id} error={error}",
                remote_actor.actor_uri
            ),
            serde_json::json!({
                "activity_type": activity_type,
                "activity_id": activity_id,
                "actor_uri": remote_actor.actor_uri,
                "target_count": accounts.len(),
                "error": error.to_string(),
            }),
        ),
    }
    inbox_result_response(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_activity_type_defaults_missing_or_non_string_type() {
        assert_eq!(
            inbox_activity_type(&serde_json::json!({"type": "Create"})),
            "Create"
        );
        assert_eq!(
            inbox_activity_type(&serde_json::json!({"type": ["Create"]})),
            "Create"
        );
        assert_eq!(inbox_activity_type(&serde_json::json!({})), "");
        assert_eq!(inbox_activity_type(&serde_json::json!({"type": 1})), "");
    }

    #[test]
    fn misskey_specific_activity_types_are_recognized_but_not_dispatched() {
        // dispatch_inbox_activity matches known Mastodon-shaped types only; Misskey
        // EmojiReact / Vote / Flag fall through to the silent Ok(()) branch.
        for activity_type in ["EmojiReact", "Vote", "Flag"] {
            assert_eq!(
                inbox_activity_type(&serde_json::json!({ "type": activity_type })),
                activity_type
            );
            assert!(!matches!(
                activity_type,
                "Follow"
                    | "Undo"
                    | "Accept"
                    | "Reject"
                    | "Like"
                    | "Create"
                    | "Announce"
                    | "Add"
                    | "Remove"
                    | "Update"
                    | "Delete"
            ));
        }
    }

    #[test]
    fn shared_inbox_unsigned_never_gets_202() {
        assert_eq!(
            shared_inbox_preprocess_outcome(false, false),
            SharedInboxPreprocessOutcome::Unauthorized
        );
        assert_eq!(
            shared_inbox_preprocess_outcome(false, true),
            SharedInboxPreprocessOutcome::Unauthorized
        );
        assert_eq!(
            shared_inbox_preprocess_outcome(true, false),
            SharedInboxPreprocessOutcome::AcceptedNoTargets
        );
        assert_eq!(
            shared_inbox_preprocess_outcome(true, true),
            SharedInboxPreprocessOutcome::Process
        );
    }
}
