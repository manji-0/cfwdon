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

use cfwdon_domain::{
    OUTBOX_DELIVERY_CONCURRENCY, generic_outbox_has_follower_targets, is_delivery_terminal,
    next_delivery_attempt_count,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub(crate) const OUTBOX_PROCESS_QUEUE_BINDING: &str = "OUTBOX_PROCESS_QUEUE";
pub(crate) const OUTBOX_IN_FLIGHT_STALE_MODIFIER: &str = "-15 minutes";

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

/// Requests under this prefix already kick the queue themselves, so re-kicking
/// them would let one drain run schedule the next one indefinitely.
const OUTBOX_KICK_EXEMPT_PATH_PREFIX: &str = "/internal/outbox/";

pub(crate) fn request_may_enqueue_outbox_work(method: &str, path: &str, status_code: u16) -> bool {
    matches!(method, "POST" | "PUT" | "PATCH" | "DELETE")
        && (200..400).contains(&status_code)
        && !path.starts_with(OUTBOX_KICK_EXEMPT_PATH_PREFIX)
}

pub(crate) async fn kick_outbox_process_queue_after_request(
    env: &Env,
    config: &AppConfig,
    method: &str,
    path: &str,
    status_code: u16,
) {
    if !request_may_enqueue_outbox_work(method, path, status_code) {
        return;
    }
    let db = match env.d1(&config.database_binding) {
        Ok(db) => db,
        Err(error) => {
            log_federation_event(
                "outbox_queue_kick_skipped",
                "failed",
                format!("outbox queue kick skipped: {error}"),
                serde_json::json!({
                    "reason": "request",
                    "error": error.to_string(),
                }),
            );
            return;
        }
    };
    if let Err(error) = enqueue_outbox_process_queue_if_pending(env, &db, "request").await {
        log_federation_event(
            "outbox_queue_kick_failed",
            "failed",
            format!("outbox queue kick failed: {error}"),
            serde_json::json!({
                "reason": "request",
                "error": error.to_string(),
            }),
        );
    }
}

pub(crate) fn outbox_batch_made_progress(summary: &OutboxProcessResponse) -> bool {
    summary.expanded > 0
        || summary.delivered > 0
        || summary.failed > 0
        || summary.completed_without_targets > 0
}

pub(crate) async fn process_outbox_deliveries_for_config(
    db: &D1Database,
    config: &AppConfig,
) -> Result<OutboxProcessResponse> {
    let mut summary = OutboxProcessResponse::default();

    requeue_stale_in_flight_deliveries(db).await?;

    process_generic_outbox_deliveries(db, &mut summary).await?;

    let (target_deliveries, outbound_deliveries) = futures_util::try_join!(
        claim_pending_target_outbox_deliveries(db, 32),
        claim_pending_outbound_activities(db, 32),
    )?;
    log_federation_event(
        "outbox_batch_candidates",
        "ok",
        format!(
            "outbox queue delivery candidates: target_deliveries={} outbound_deliveries={}",
            target_deliveries.len(),
            outbound_deliveries.len()
        ),
        serde_json::json!({
            "target_deliveries": target_deliveries.len(),
            "outbound_deliveries": outbound_deliveries.len(),
        }),
    );
    let mut account_cache = HashMap::new();
    let mut blocked_domains_cache = HashMap::<String, Vec<String>>::new();
    preload_outbox_delivery_accounts(db, config, &target_deliveries, &mut account_cache).await?;
    preload_outbound_activity_accounts(db, config, &outbound_deliveries, &mut account_cache)
        .await?;

    let mut target_send_jobs = Vec::new();
    for delivery in target_deliveries {
        let Some(target_inbox) = delivery.target_inbox.clone() else {
            continue;
        };
        let Some(account) = account_cache.get(&delivery.account_id) else {
            log_federation_event(
                "outbox_delivery_failed",
                "failed",
                format!(
                    "outbox delivery failed: id={} target={} attempt={} terminal=true error=missing account {}",
                    delivery.id,
                    target_inbox,
                    delivery.attempt_count.saturating_add(1),
                    delivery.account_id
                ),
                serde_json::json!({
                    "channel": "outbox_deliveries",
                    "delivery_id": delivery.id,
                    "account_id": delivery.account_id,
                    "activity_id": delivery.activity_id,
                    "activity_type": delivery.activity_type,
                    "target_inbox": target_inbox,
                    "attempt": delivery.attempt_count.saturating_add(1),
                    "terminal": true,
                    "permanent": true,
                    "error": format!("missing account {}", delivery.account_id),
                }),
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
        if !blocked_domains_cache.contains_key(&delivery.account_id) {
            let blocked = list_all_account_domain_blocks(db, &delivery.account_id).await?;
            blocked_domains_cache.insert(delivery.account_id.clone(), blocked);
        }
        let blocked_domains = blocked_domains_cache
            .get(&delivery.account_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if delivery_inbox_blocked_by_domains(&target_inbox, blocked_domains) {
            log_federation_event(
                "outbox_delivery_skipped",
                "skipped",
                format!(
                    "outbox delivery skipped domain block: id={} target={}",
                    delivery.id, target_inbox
                ),
                serde_json::json!({
                    "channel": "outbox_deliveries",
                    "delivery_id": delivery.id,
                    "account_id": delivery.account_id,
                    "activity_id": delivery.activity_id,
                    "activity_type": delivery.activity_type,
                    "target_inbox": target_inbox,
                    "reason": "domain_block",
                }),
            );
            if mark_outbox_delivery_terminal_failure(
                db,
                &delivery.id,
                delivery.attempt_count.saturating_add(1) as u32,
            )
            .await?
            {
                summary.failed += 1;
            }
            continue;
        }
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
                if mark_outbox_delivery_delivered(db, &delivery.id).await? {
                    summary.delivered += 1;
                    log_federation_event(
                        "outbox_delivery_delivered",
                        "delivered",
                        format!(
                            "outbox delivery delivered: id={} target={} activity_type={}",
                            delivery.id, target_inbox, delivery.activity_type
                        ),
                        serde_json::json!({
                            "channel": "outbox_deliveries",
                            "delivery_id": delivery.id,
                            "account_id": delivery.account_id,
                            "activity_id": delivery.activity_id,
                            "activity_type": delivery.activity_type,
                            "target_inbox": target_inbox,
                            "attempt": delivery.attempt_count,
                        }),
                    );
                } else {
                    log_federation_event(
                        "outbox_delivery_mark_noop",
                        "failed",
                        format!(
                            "outbox delivery mark delivered no-op: id={} target={}",
                            delivery.id, target_inbox
                        ),
                        serde_json::json!({
                            "channel": "outbox_deliveries",
                            "delivery_id": delivery.id,
                            "target_inbox": target_inbox,
                            "error": "mark_delivered_noop",
                        }),
                    );
                }
            }
            Err(error) => {
                let next_attempt = next_delivery_attempt_count(delivery.attempt_count);
                let permanent = error.is_permanent();
                let terminal = permanent || is_delivery_terminal(next_attempt);
                log_federation_event(
                    "outbox_delivery_failed",
                    "failed",
                    format!(
                        "outbox delivery failed: id={} target={} attempt={} terminal={} permanent={} error={}",
                        delivery.id, target_inbox, next_attempt, terminal, permanent, error.detail
                    ),
                    serde_json::json!({
                        "channel": "outbox_deliveries",
                        "delivery_id": delivery.id,
                        "account_id": delivery.account_id,
                        "activity_id": delivery.activity_id,
                        "activity_type": delivery.activity_type,
                        "target_inbox": target_inbox,
                        "attempt": next_attempt,
                        "terminal": terminal,
                        "permanent": permanent,
                        "error": error.detail,
                    }),
                );
                if terminal {
                    if mark_outbox_delivery_terminal_failure(db, &delivery.id, next_attempt).await?
                    {
                        summary.failed += 1;
                    }
                } else if reschedule_outbox_delivery(db, &delivery.id, next_attempt).await? {
                    summary.failed += 1;
                }
            }
        }
    }

    let mut outbound_send_jobs = Vec::new();
    for delivery in outbound_deliveries {
        let Some(account) = account_cache.get(&delivery.account_id) else {
            log_federation_event(
                "outbox_delivery_failed",
                "failed",
                format!(
                    "outbound delivery failed: id={} target={} attempt={} terminal=true error=missing account {}",
                    delivery.id,
                    delivery.target_inbox,
                    delivery.attempt_count.saturating_add(1),
                    delivery.account_id
                ),
                serde_json::json!({
                    "channel": "outbound_activities",
                    "delivery_id": delivery.id,
                    "account_id": delivery.account_id,
                    "activity_id": delivery.activity_id,
                    "activity_type": delivery.activity_type,
                    "target_inbox": delivery.target_inbox,
                    "target_actor_uri": delivery.target_actor_uri,
                    "attempt": delivery.attempt_count.saturating_add(1),
                    "terminal": true,
                    "permanent": true,
                    "error": format!("missing account {}", delivery.account_id),
                }),
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
        if !blocked_domains_cache.contains_key(&delivery.account_id) {
            let blocked = list_all_account_domain_blocks(db, &delivery.account_id).await?;
            blocked_domains_cache.insert(delivery.account_id.clone(), blocked);
        }
        let blocked_domains = blocked_domains_cache
            .get(&delivery.account_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if delivery_inbox_blocked_by_domains(&delivery.target_inbox, blocked_domains) {
            log_federation_event(
                "outbox_delivery_skipped",
                "skipped",
                format!(
                    "outbound delivery skipped domain block: id={} target={}",
                    delivery.id, delivery.target_inbox
                ),
                serde_json::json!({
                    "channel": "outbound_activities",
                    "delivery_id": delivery.id,
                    "account_id": delivery.account_id,
                    "activity_id": delivery.activity_id,
                    "activity_type": delivery.activity_type,
                    "target_inbox": delivery.target_inbox,
                    "target_actor_uri": delivery.target_actor_uri,
                    "reason": "domain_block",
                }),
            );
            reconcile_outbound_activity_terminal_failure(
                db,
                &delivery,
                delivery.attempt_count.saturating_add(1) as u32,
            )
            .await?;
            summary.failed += 1;
            continue;
        }
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
                if mark_outbound_activity_delivered(db, &delivery.id).await? {
                    summary.delivered += 1;
                    log_federation_event(
                        "outbox_delivery_delivered",
                        "delivered",
                        format!(
                            "outbound delivery delivered: id={} target={} activity_type={}",
                            delivery.id, delivery.target_inbox, delivery.activity_type
                        ),
                        serde_json::json!({
                            "channel": "outbound_activities",
                            "delivery_id": delivery.id,
                            "account_id": delivery.account_id,
                            "activity_id": delivery.activity_id,
                            "activity_type": delivery.activity_type,
                            "target_inbox": delivery.target_inbox,
                            "target_actor_uri": delivery.target_actor_uri,
                            "attempt": delivery.attempt_count,
                        }),
                    );
                } else {
                    log_federation_event(
                        "outbox_delivery_mark_noop",
                        "failed",
                        format!(
                            "outbound delivery mark delivered no-op: id={} target={}",
                            delivery.id, delivery.target_inbox
                        ),
                        serde_json::json!({
                            "channel": "outbound_activities",
                            "delivery_id": delivery.id,
                            "target_inbox": delivery.target_inbox,
                            "error": "mark_delivered_noop",
                        }),
                    );
                }
            }
            Err(error) => {
                let next_attempt = next_delivery_attempt_count(delivery.attempt_count);
                let permanent = error.is_permanent();
                let terminal = permanent || is_delivery_terminal(next_attempt);
                log_federation_event(
                    "outbox_delivery_failed",
                    "failed",
                    format!(
                        "outbound delivery failed: id={} target={} attempt={} terminal={} permanent={} error={}",
                        delivery.id,
                        delivery.target_inbox,
                        next_attempt,
                        terminal,
                        permanent,
                        error.detail
                    ),
                    serde_json::json!({
                        "channel": "outbound_activities",
                        "delivery_id": delivery.id,
                        "account_id": delivery.account_id,
                        "activity_id": delivery.activity_id,
                        "activity_type": delivery.activity_type,
                        "target_inbox": delivery.target_inbox,
                        "target_actor_uri": delivery.target_actor_uri,
                        "attempt": next_attempt,
                        "terminal": terminal,
                        "permanent": permanent,
                        "error": error.detail,
                    }),
                );
                if terminal {
                    reconcile_outbound_activity_terminal_failure(db, &delivery, next_attempt)
                        .await?;
                    summary.failed += 1;
                } else if reschedule_outbound_activity(db, &delivery.id, next_attempt).await? {
                    summary.failed += 1;
                }
            }
        }
    }

    Ok(summary)
}

async fn requeue_stale_in_flight_deliveries(db: &D1Database) -> Result<()> {
    requeue_stale_in_flight_outbox_deliveries(db).await?;
    requeue_stale_in_flight_outbound_activities(db).await?;
    Ok(())
}

async fn process_generic_outbox_deliveries(
    db: &D1Database,
    summary: &mut OutboxProcessResponse,
) -> Result<()> {
    let deliveries = claim_pending_generic_outbox_deliveries(db, 16).await?;
    if deliveries.is_empty() {
        return Ok(());
    }

    let account_ids = deliveries
        .iter()
        .map(|delivery| delivery.account_id.clone())
        .collect::<Vec<_>>();
    let targets_by_account =
        list_follower_delivery_targets_by_account_ids(db, &account_ids).await?;
    let mut filtered_targets = HashMap::new();
    for (account_id, targets) in targets_by_account {
        let blocked_domains = list_all_account_domain_blocks(db, &account_id).await?;
        let targets = filter_delivery_inboxes_for_domain_blocks(targets, &blocked_domains);
        if !targets.is_empty() {
            filtered_targets.insert(account_id, targets);
        }
    }
    let targets_by_account = filtered_targets;
    let target_count = targets_by_account
        .values()
        .map(std::vec::Vec::len)
        .sum::<usize>();

    let (deliveries_with_targets, completed_without_targets) =
        partition_generic_outbox_deliveries_by_targets(&deliveries, &targets_by_account);
    log_federation_event(
        "outbox_expand_planned",
        "ok",
        format!(
            "outbox generic delivery expansion: pending={} accounts={} accounts_with_targets={} targets={} with_targets={} without_targets={}",
            deliveries.len(),
            unique_ordered_refs(&account_ids).len(),
            targets_by_account.len(),
            target_count,
            deliveries_with_targets.len(),
            completed_without_targets.len()
        ),
        serde_json::json!({
            "pending": deliveries.len(),
            "accounts": unique_ordered_refs(&account_ids).len(),
            "accounts_with_targets": targets_by_account.len(),
            "targets": target_count,
            "with_targets": deliveries_with_targets.len(),
            "without_targets": completed_without_targets.len(),
        }),
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
    log_federation_event(
        "outbox_expanded",
        "ok",
        format!("outbox generic delivery expanded target rows: expanded={expanded_count}"),
        serde_json::json!({
            "expanded": expanded_count,
        }),
    );
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
