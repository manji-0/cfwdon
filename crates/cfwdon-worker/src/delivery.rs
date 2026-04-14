use super::{
    Request, Response, Result, RouteContext, ensure_account_keys, expand_outbox_delivery_targets,
    extract_authenticated_user, find_account_by_id, list_follower_delivery_targets,
    list_pending_generic_outbox_deliveries, list_pending_outbound_activities,
    list_pending_target_outbox_deliveries, load_config, mark_outbound_activity_delivered,
    mark_outbox_delivery_completed_without_targets, mark_outbox_delivery_delivered,
    mark_outbox_delivery_expanded, mark_outbox_delivery_terminal_failure,
    reconcile_outbound_activity_terminal_failure, reschedule_outbound_activity,
    reschedule_outbox_delivery, send_signed_activity,
};
use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub(crate) struct OutboxProcessResponse {
    pub(crate) expanded: u32,
    pub(crate) delivered: u32,
    pub(crate) failed: u32,
    pub(crate) completed_without_targets: u32,
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
    let mut summary = OutboxProcessResponse::default();

    for delivery in list_pending_generic_outbox_deliveries(&db, 16).await? {
        let targets = list_follower_delivery_targets(&db, &delivery.account_id).await?;
        if targets.is_empty() {
            mark_outbox_delivery_completed_without_targets(&db, &delivery.id).await?;
            summary.completed_without_targets += 1;
            continue;
        }

        summary.expanded += expand_outbox_delivery_targets(&db, &delivery, &targets).await? as u32;
        mark_outbox_delivery_expanded(&db, &delivery.id).await?;
    }

    for delivery in list_pending_target_outbox_deliveries(&db, 32).await? {
        let Some(target_inbox) = delivery.target_inbox.as_deref() else {
            continue;
        };
        let Some(account) = find_account_by_id(&db, &delivery.account_id).await? else {
            mark_outbox_delivery_terminal_failure(
                &db,
                &delivery.id,
                delivery.attempt_count.saturating_add(1) as u32,
            )
            .await?;
            summary.failed += 1;
            continue;
        };
        let account = ensure_account_keys(&db, account).await?;

        match send_signed_activity(&config, &account, target_inbox, &delivery.payload_json).await {
            Ok(()) => {
                mark_outbox_delivery_delivered(&db, &delivery.id).await?;
                summary.delivered += 1;
            }
            Err(_) => {
                let next_attempt = delivery.attempt_count.saturating_add(1) as u32;
                if next_attempt >= 5 {
                    mark_outbox_delivery_terminal_failure(&db, &delivery.id, next_attempt).await?;
                } else {
                    reschedule_outbox_delivery(&db, &delivery.id, next_attempt).await?;
                }
                summary.failed += 1;
            }
        }
    }

    for delivery in list_pending_outbound_activities(&db, 32).await? {
        let Some(account) = find_account_by_id(&db, &delivery.account_id).await? else {
            reconcile_outbound_activity_terminal_failure(
                &db,
                &delivery,
                delivery.attempt_count.saturating_add(1) as u32,
            )
            .await?;
            summary.failed += 1;
            continue;
        };
        let account = ensure_account_keys(&db, account).await?;

        match send_signed_activity(
            &config,
            &account,
            &delivery.target_inbox,
            &delivery.payload_json,
        )
        .await
        {
            Ok(()) => {
                mark_outbound_activity_delivered(&db, &delivery.id).await?;
                summary.delivered += 1;
            }
            Err(_) => {
                let next_attempt = delivery.attempt_count.saturating_add(1) as u32;
                if next_attempt >= 5 {
                    reconcile_outbound_activity_terminal_failure(&db, &delivery, next_attempt)
                        .await?;
                } else {
                    reschedule_outbound_activity(&db, &delivery.id, next_attempt).await?;
                }
                summary.failed += 1;
            }
        }
    }

    Response::from_json(&summary)
}
