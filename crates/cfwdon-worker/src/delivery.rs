#[allow(unused_imports)]
pub(crate) use crate::*;

mod outbound;
mod outbound_state;
mod outbox_enqueue;
mod store;
mod store_state;
pub(crate) use outbound::*;
pub(crate) use outbound_state::*;
pub(crate) use outbox_enqueue::*;
pub(crate) use store::*;
pub(crate) use store_state::*;

use serde::{Deserialize, Serialize};

pub(crate) const OUTBOX_PROCESS_QUEUE_BINDING: &str = "OUTBOX_PROCESS_QUEUE";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct OutboxProcessQueueMessage {
    pub(crate) reason: String,
}

impl OutboxProcessQueueMessage {
    pub(crate) fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct OutboxProcessResponse {
    pub(crate) expanded: u32,
    pub(crate) delivered: u32,
    pub(crate) failed: u32,
    pub(crate) completed_without_targets: u32,
}

#[derive(Debug, Serialize)]
struct OutboxProcessKickResponse {
    status: &'static str,
    queued: bool,
}

pub(crate) async fn process_outbox_deliveries(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    match extract_authenticated_user(&req, &config).await? {
        Some(_) => {}
        None => return Response::error("Cloudflare Access authentication required", 401),
    }

    let db = ctx.d1(&config.database_binding)?;
    if !pending_outbox_work_exists(&db).await? {
        return Response::from_json(&OutboxProcessKickResponse {
            status: "idle",
            queued: false,
        });
    }

    enqueue_outbox_process_queue(&ctx.env, "internal_outbox_process").await?;
    Ok(Response::from_json(&OutboxProcessKickResponse {
        status: "queued",
        queued: true,
    })?
    .with_status(202))
}

pub(crate) async fn enqueue_outbox_process_queue(env: &Env, reason: &str) -> Result<()> {
    let queue = env.queue(OUTBOX_PROCESS_QUEUE_BINDING)?;
    queue
        .send(OutboxProcessQueueMessage::new(reason.to_owned()))
        .await
}

pub(crate) async fn enqueue_outbox_process_queue_if_pending(
    env: &Env,
    db: &D1Database,
    reason: &str,
) -> Result<bool> {
    if !pending_outbox_work_exists(db).await? {
        return Ok(false);
    }
    enqueue_outbox_process_queue(env, reason).await?;
    Ok(true)
}

pub(crate) async fn enqueue_outbox_process_queue_best_effort(env: &Env, reason: &str) {
    if let Err(error) = enqueue_outbox_process_queue(env, reason).await {
        console_error!("failed to enqueue outbox processing: {error}");
    }
}

pub(crate) async fn process_outbox_deliveries_for_config(
    db: &D1Database,
    config: &AppConfig,
) -> Result<OutboxProcessResponse> {
    let mut summary = OutboxProcessResponse::default();

    for delivery in list_pending_generic_outbox_deliveries(db, 16).await? {
        let targets = list_follower_delivery_targets(db, &delivery.account_id).await?;
        if targets.is_empty() {
            mark_outbox_delivery_completed_without_targets(db, &delivery.id).await?;
            summary.completed_without_targets += 1;
            continue;
        }

        summary.expanded += expand_outbox_delivery_targets(db, &delivery, &targets).await? as u32;
        mark_outbox_delivery_expanded(db, &delivery.id).await?;
    }

    for delivery in list_pending_target_outbox_deliveries(db, 32).await? {
        let Some(target_inbox) = delivery.target_inbox.as_deref() else {
            continue;
        };
        let Some(account) = find_account_by_id(db, &delivery.account_id).await? else {
            mark_outbox_delivery_terminal_failure(
                db,
                &delivery.id,
                delivery.attempt_count.saturating_add(1) as u32,
            )
            .await?;
            summary.failed += 1;
            continue;
        };
        let account = ensure_account_keys(db, account).await?;

        match send_signed_activity(config, &account, target_inbox, &delivery.payload_json).await {
            Ok(()) => {
                mark_outbox_delivery_delivered(db, &delivery.id).await?;
                summary.delivered += 1;
            }
            Err(_) => {
                let next_attempt = delivery.attempt_count.saturating_add(1) as u32;
                if next_attempt >= 5 {
                    mark_outbox_delivery_terminal_failure(db, &delivery.id, next_attempt).await?;
                } else {
                    reschedule_outbox_delivery(db, &delivery.id, next_attempt).await?;
                }
                summary.failed += 1;
            }
        }
    }

    for delivery in list_pending_outbound_activities(db, 32).await? {
        let Some(account) = find_account_by_id(db, &delivery.account_id).await? else {
            reconcile_outbound_activity_terminal_failure(
                db,
                &delivery,
                delivery.attempt_count.saturating_add(1) as u32,
            )
            .await?;
            summary.failed += 1;
            continue;
        };
        let account = ensure_account_keys(db, account).await?;

        match send_signed_activity(
            config,
            &account,
            &delivery.target_inbox,
            &delivery.payload_json,
        )
        .await
        {
            Ok(()) => {
                mark_outbound_activity_delivered(db, &delivery.id).await?;
                summary.delivered += 1;
            }
            Err(_) => {
                let next_attempt = delivery.attempt_count.saturating_add(1) as u32;
                if next_attempt >= 5 {
                    reconcile_outbound_activity_terminal_failure(db, &delivery, next_attempt)
                        .await?;
                } else {
                    reschedule_outbound_activity(db, &delivery.id, next_attempt).await?;
                }
                summary.failed += 1;
            }
        }
    }

    Ok(summary)
}
