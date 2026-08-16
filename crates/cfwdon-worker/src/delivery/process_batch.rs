use super::{
    OutboundActivityRow, OutboxDeliveryRow, claim_pending_outbound_activities,
    claim_pending_target_outbox_deliveries, mark_outbound_activity_delivered,
    mark_outbox_delivery_delivered, mark_outbox_delivery_terminal_failure,
    reconcile_outbound_activity_terminal_failure, reschedule_outbound_activity,
    reschedule_outbox_delivery,
};
use super::{
    OutboxProcessResponse, preload_outbound_activity_accounts, preload_outbox_delivery_accounts,
    process_generic_outbox_deliveries, requeue_stale_in_flight_deliveries,
};
use crate::{
    AppConfig, D1Database, LocalAccount, Result, delivery_inbox_blocked_by_domains,
    list_all_account_domain_blocks, log_federation_event, send_signed_activity,
};
use cfwdon_domain::{
    OUTBOX_DELIVERY_CONCURRENCY, is_delivery_terminal, next_delivery_attempt_count,
};
use futures_util::StreamExt;
use std::collections::HashMap;

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

    process_claimed_target_outbox_deliveries(
        db,
        config,
        &mut summary,
        target_deliveries,
        &account_cache,
        &mut blocked_domains_cache,
    )
    .await?;
    process_claimed_outbound_activities(
        db,
        config,
        &mut summary,
        outbound_deliveries,
        &account_cache,
        &mut blocked_domains_cache,
    )
    .await?;

    Ok(summary)
}

async fn blocked_domains_for_account(
    db: &D1Database,
    account_id: &str,
    blocked_domains_cache: &mut HashMap<String, Vec<String>>,
) -> Result<Vec<String>> {
    if !blocked_domains_cache.contains_key(account_id) {
        let blocked = list_all_account_domain_blocks(db, account_id).await?;
        blocked_domains_cache.insert(account_id.to_owned(), blocked);
    }
    Ok(blocked_domains_cache
        .get(account_id)
        .cloned()
        .unwrap_or_default())
}

async fn process_claimed_target_outbox_deliveries(
    db: &D1Database,
    config: &AppConfig,
    summary: &mut OutboxProcessResponse,
    target_deliveries: Vec<OutboxDeliveryRow>,
    account_cache: &HashMap<String, LocalAccount>,
    blocked_domains_cache: &mut HashMap<String, Vec<String>>,
) -> Result<()> {
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
        let blocked_domains =
            blocked_domains_for_account(db, &delivery.account_id, blocked_domains_cache).await?;
        if delivery_inbox_blocked_by_domains(&target_inbox, &blocked_domains) {
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

    Ok(())
}

async fn process_claimed_outbound_activities(
    db: &D1Database,
    config: &AppConfig,
    summary: &mut OutboxProcessResponse,
    outbound_deliveries: Vec<OutboundActivityRow>,
    account_cache: &HashMap<String, LocalAccount>,
    blocked_domains_cache: &mut HashMap<String, Vec<String>>,
) -> Result<()> {
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
        let blocked_domains =
            blocked_domains_for_account(db, &delivery.account_id, blocked_domains_cache).await?;
        if delivery_inbox_blocked_by_domains(&delivery.target_inbox, &blocked_domains) {
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

    Ok(())
}
