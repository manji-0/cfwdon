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

use cfwdon_domain::{OUTBOX_DELIVERY_CONCURRENCY, generic_outbox_has_follower_targets};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
        None => return Response::error("Auth0 authentication required", 401),
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

    process_generic_outbox_deliveries(db, &mut summary).await?;

    let (target_deliveries, outbound_deliveries) = futures_util::try_join!(
        list_pending_target_outbox_deliveries(db, 32),
        list_pending_outbound_activities(db, 32),
    )?;
    console_log!(
        "outbox queue delivery candidates: target_deliveries={} outbound_deliveries={}",
        target_deliveries.len(),
        outbound_deliveries.len()
    );
    let mut account_cache = HashMap::new();
    preload_outbox_delivery_accounts(db, config, &target_deliveries, &mut account_cache).await?;
    preload_outbound_activity_accounts(db, config, &outbound_deliveries, &mut account_cache)
        .await?;

    let mut target_send_jobs = Vec::new();
    for delivery in target_deliveries {
        let Some(target_inbox) = delivery.target_inbox.clone() else {
            continue;
        };
        let Some(account) = account_cache.get(&delivery.account_id) else {
            console_error!(
                "outbox delivery failed: id={} target={} attempt={} terminal=true error=missing account {}",
                delivery.id,
                target_inbox,
                delivery.attempt_count.saturating_add(1),
                delivery.account_id
            );
            mark_outbox_delivery_terminal_failure(
                db,
                &delivery.id,
                delivery.attempt_count.saturating_add(1) as u32,
            )
            .await?;
            summary.failed += 1;
            continue;
        };
        target_send_jobs.push((delivery, target_inbox, account.clone()));
    }

    let mut target_results = futures_util::stream::iter(target_send_jobs)
        .map(|(delivery, target_inbox, account)| async move {
            let result =
                send_signed_activity(config, db, &account, &target_inbox, &delivery.payload_json)
                    .await;
            (delivery, target_inbox, result)
        })
        .buffer_unordered(OUTBOX_DELIVERY_CONCURRENCY);
    while let Some((delivery, target_inbox, result)) = target_results.next().await {
        match result {
            Ok(()) => {
                mark_outbox_delivery_delivered(db, &delivery.id).await?;
                summary.delivered += 1;
            }
            Err(error) => {
                let next_attempt = delivery.attempt_count.saturating_add(1) as u32;
                console_error!(
                    "outbox delivery failed: id={} target={} attempt={} terminal={} error={}",
                    delivery.id,
                    target_inbox,
                    next_attempt,
                    next_attempt >= 5,
                    error
                );
                if next_attempt >= 5 {
                    mark_outbox_delivery_terminal_failure(db, &delivery.id, next_attempt).await?;
                } else {
                    reschedule_outbox_delivery(db, &delivery.id, next_attempt).await?;
                }
                summary.failed += 1;
            }
        }
    }

    let mut outbound_send_jobs = Vec::new();
    for delivery in outbound_deliveries {
        let Some(account) = account_cache.get(&delivery.account_id) else {
            console_error!(
                "outbound delivery failed: id={} target={} attempt={} terminal=true error=missing account {}",
                delivery.id,
                delivery.target_inbox,
                delivery.attempt_count.saturating_add(1),
                delivery.account_id
            );
            reconcile_outbound_activity_terminal_failure(
                db,
                &delivery,
                delivery.attempt_count.saturating_add(1) as u32,
            )
            .await?;
            summary.failed += 1;
            continue;
        };
        outbound_send_jobs.push((delivery, account.clone()));
    }

    let mut outbound_results = futures_util::stream::iter(outbound_send_jobs)
        .map(|(delivery, account)| async move {
            let result = send_signed_activity(
                config,
                db,
                &account,
                &delivery.target_inbox,
                &delivery.payload_json,
            )
            .await;
            (delivery, result)
        })
        .buffer_unordered(OUTBOX_DELIVERY_CONCURRENCY);
    while let Some((delivery, result)) = outbound_results.next().await {
        match result {
            Ok(()) => {
                mark_outbound_activity_delivered(db, &delivery.id).await?;
                summary.delivered += 1;
            }
            Err(error) => {
                let next_attempt = delivery.attempt_count.saturating_add(1) as u32;
                console_error!(
                    "outbound delivery failed: id={} target={} attempt={} terminal={} error={}",
                    delivery.id,
                    delivery.target_inbox,
                    next_attempt,
                    next_attempt >= 5,
                    error
                );
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

async fn process_generic_outbox_deliveries(
    db: &D1Database,
    summary: &mut OutboxProcessResponse,
) -> Result<()> {
    let deliveries = list_pending_generic_outbox_deliveries(db, 16).await?;
    if deliveries.is_empty() {
        return Ok(());
    }

    let account_ids = deliveries
        .iter()
        .map(|delivery| delivery.account_id.clone())
        .collect::<Vec<_>>();
    let targets_by_account =
        list_follower_delivery_targets_by_account_ids(db, &account_ids).await?;
    let target_count = targets_by_account
        .values()
        .map(std::vec::Vec::len)
        .sum::<usize>();

    let (deliveries_with_targets, completed_without_targets) =
        partition_generic_outbox_deliveries_by_targets(&deliveries, &targets_by_account);
    console_log!(
        "outbox generic delivery expansion: pending={} accounts={} accounts_with_targets={} targets={} with_targets={} without_targets={}",
        deliveries.len(),
        unique_ordered_refs(&account_ids).len(),
        targets_by_account.len(),
        target_count,
        deliveries_with_targets.len(),
        completed_without_targets.len()
    );

    if !completed_without_targets.is_empty() {
        mark_outbox_deliveries_completed_without_targets(db, &completed_without_targets).await?;
        summary.completed_without_targets += completed_without_targets.len() as u32;
    }

    if deliveries_with_targets.is_empty() {
        return Ok(());
    }

    let expanded_count = expand_outbox_delivery_targets_for_deliveries(
        db,
        &deliveries_with_targets,
        &targets_by_account,
    )
    .await? as u32;
    console_log!("outbox generic delivery expanded target rows: expanded={expanded_count}");
    summary.expanded += expanded_count;
    let expanded_ids = deliveries_with_targets
        .iter()
        .map(|delivery| delivery.id.clone())
        .collect::<Vec<_>>();
    mark_outbox_deliveries_expanded(db, &expanded_ids).await?;
    Ok(())
}

fn partition_generic_outbox_deliveries_by_targets<'a>(
    deliveries: &'a [OutboxDeliveryRow],
    targets_by_account: &HashMap<String, Vec<String>>,
) -> (Vec<&'a OutboxDeliveryRow>, Vec<String>) {
    let mut deliveries_with_targets = Vec::new();
    let mut completed_without_targets = Vec::new();
    for delivery in deliveries {
        match targets_by_account.get(&delivery.account_id) {
            Some(targets) if generic_outbox_has_follower_targets(targets.len()) => {
                deliveries_with_targets.push(delivery)
            }
            _ => completed_without_targets.push(delivery.id.clone()),
        }
    }
    (deliveries_with_targets, completed_without_targets)
}

async fn preload_outbox_delivery_accounts(
    db: &D1Database,
    config: &AppConfig,
    deliveries: &[OutboxDeliveryRow],
    account_cache: &mut HashMap<String, LocalAccount>,
) -> Result<()> {
    let missing_account_ids = deliveries
        .iter()
        .filter(|delivery| !account_cache.contains_key(&delivery.account_id))
        .map(|delivery| delivery.account_id.clone())
        .collect::<Vec<_>>();
    preload_delivery_accounts(db, config, &missing_account_ids, account_cache).await
}

async fn preload_outbound_activity_accounts(
    db: &D1Database,
    config: &AppConfig,
    deliveries: &[OutboundActivityRow],
    account_cache: &mut HashMap<String, LocalAccount>,
) -> Result<()> {
    let missing_account_ids = deliveries
        .iter()
        .filter(|delivery| !account_cache.contains_key(&delivery.account_id))
        .map(|delivery| delivery.account_id.clone())
        .collect::<Vec<_>>();
    preload_delivery_accounts(db, config, &missing_account_ids, account_cache).await
}

async fn preload_delivery_accounts(
    db: &D1Database,
    config: &AppConfig,
    account_ids: &[String],
    account_cache: &mut HashMap<String, LocalAccount>,
) -> Result<()> {
    let accounts = find_accounts_by_ids(db, account_ids).await?;
    for (account_id, account) in accounts {
        let account = ensure_account_keys(db, config, account).await?;
        account_cache.insert(account_id, account);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outbox_delivery(id: &str, account_id: &str) -> OutboxDeliveryRow {
        OutboxDeliveryRow {
            id: id.to_owned(),
            account_id: account_id.to_owned(),
            status_id: format!("status-{id}"),
            activity_id: format!("activity-{id}"),
            activity_type: "Create".to_owned(),
            target_inbox: None,
            payload_json: "{}".to_owned(),
            attempt_count: 0,
        }
    }

    #[test]
    fn partition_generic_outbox_deliveries_keeps_deliveries_with_targets() {
        let deliveries = vec![
            outbox_delivery("delivery-1", "account-1"),
            outbox_delivery("delivery-2", "account-2"),
            outbox_delivery("delivery-3", "account-3"),
        ];
        let targets_by_account = HashMap::from([
            (
                "account-1".to_owned(),
                vec!["https://remote.example/inbox".to_owned()],
            ),
            ("account-2".to_owned(), Vec::new()),
        ]);

        let (with_targets, without_targets) =
            partition_generic_outbox_deliveries_by_targets(&deliveries, &targets_by_account);

        assert_eq!(
            with_targets
                .into_iter()
                .map(|delivery| delivery.id.as_str())
                .collect::<Vec<_>>(),
            vec!["delivery-1"]
        );
        assert_eq!(without_targets, vec!["delivery-2", "delivery-3"]);
    }
}
