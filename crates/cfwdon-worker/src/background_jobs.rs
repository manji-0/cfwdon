use crate::{
    AppConfig, D1Database, build_remote_status_card_value, build_status_card_value,
    enrich_card_with_remote_preview, find_remote_status_by_id,
    find_remote_status_by_url_or_object_uri, find_status_by_id, generate_entity_id, now_iso_string,
    resolve_remote_status_by_url,
};
use serde::Deserialize;
use worker::{Env, Error, Result, d1::D1Type};

pub(crate) const JOB_CARD_UNFURL: &str = "card_unfurl";
pub(crate) const JOB_RESOLVE_IN_REPLY_TO: &str = "resolve_in_reply_to";
pub(crate) const JOB_REMOTE_CONTEXT_FETCH: &str = "remote_context_fetch";
pub(crate) const JOB_REMOTE_STATUS_NOTIFY: &str = "remote_status_notify";

const MAX_ATTEMPTS: i32 = 5;
const BASE_BACKOFF_SECS: i64 = 60;

pub(crate) async fn enqueue_background_job(
    db: &D1Database,
    job_type: &str,
    payload_json: &str,
    next_run_at: &str,
) -> Result<()> {
    let id = generate_entity_id(16)?;
    let now = now_iso_string()?;
    let bindings = [
        D1Type::Text(&id),
        D1Type::Text(job_type),
        D1Type::Text(payload_json),
        D1Type::Text(next_run_at),
        D1Type::Text(&now),
    ];
    db.prepare(
        "INSERT OR IGNORE INTO background_jobs
         (id, job_type, payload_json, status, attempts, next_run_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'pending', 0, ?4, ?5, ?5)",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

/// Soft-enqueue: if a pending job for the same type+payload already exists,
/// skip insertion to avoid duplicate work.
pub(crate) async fn soft_enqueue_background_job(
    db: &D1Database,
    job_type: &str,
    payload_json: &str,
    next_run_at: &str,
) -> Result<()> {
    let bindings_check = [D1Type::Text(job_type), D1Type::Text(payload_json)];
    let already_pending = db
        .prepare(
            "SELECT 1 AS found
             FROM background_jobs
             WHERE job_type = ?1
               AND payload_json = ?2
               AND status IN ('pending', 'running')
             LIMIT 1",
        )
        .bind_refs(bindings_check.iter())?
        .first::<serde_json::Value>(None)
        .await?
        .is_some();

    if !already_pending {
        enqueue_background_job(db, job_type, payload_json, next_run_at).await?;
    }
    Ok(())
}

#[derive(Deserialize)]
struct BackgroundJobRow {
    id: String,
    job_type: String,
    payload_json: String,
    attempts: i32,
}

pub(crate) async fn process_due_background_jobs(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
    limit: u32,
) -> Result<u32> {
    let now = now_iso_string()?;
    let limit_val = i32::try_from(limit).unwrap_or(i32::MAX);

    // Claim pending jobs.
    let claim_bindings = [D1Type::Text(&now), D1Type::Integer(limit_val)];
    let job_ids_result = db
        .prepare(
            "SELECT id, job_type, payload_json, attempts
             FROM background_jobs
             WHERE status = 'pending'
               AND next_run_at <= ?1
             ORDER BY next_run_at ASC
             LIMIT ?2",
        )
        .bind_refs(claim_bindings.iter())?
        .all()
        .await?;

    let jobs = job_ids_result.results::<BackgroundJobRow>()?;
    if jobs.is_empty() {
        return Ok(0);
    }

    let ids_json =
        crate::json_string_array(&jobs.iter().map(|j| j.id.as_str()).collect::<Vec<_>>());
    let ids_binding = D1Type::Text(&ids_json);
    db.prepare(format!(
        "UPDATE background_jobs SET status = 'running', updated_at = ?1
         WHERE id {}",
        crate::sql_in_json_each(2)
    ))
    .bind_refs(&[D1Type::Text(&now), ids_binding])?
    .run()
    .await?;

    let mut processed = 0u32;
    for job in jobs {
        let result = handle_job(db, config, env, &job.job_type, &job.payload_json).await;
        match result {
            Ok(()) => {
                let finish_bindings = [D1Type::Text(&job.id), D1Type::Text(&now)];
                let _ = db
                    .prepare(
                        "UPDATE background_jobs
                         SET status = 'completed', updated_at = ?2
                         WHERE id = ?1",
                    )
                    .bind_refs(finish_bindings.iter())?
                    .run()
                    .await;
                processed += 1;
            }
            Err(err) => {
                let next_attempts = job.attempts + 1;
                let (new_status, next_run_at) = if next_attempts >= MAX_ATTEMPTS {
                    ("failed".to_owned(), now.clone())
                } else {
                    let backoff_secs = BASE_BACKOFF_SECS * (1i64 << next_attempts.min(6));
                    let backoff_secs_u64 =
                        u64::try_from(backoff_secs).unwrap_or(BASE_BACKOFF_SECS as u64);
                    let next = crate::add_seconds_to_iso_string(&now, backoff_secs_u64)
                        .unwrap_or(now.clone());
                    ("pending".to_owned(), next)
                };
                let error_msg = err.to_string();
                let retry_bindings = [
                    D1Type::Text(&new_status),
                    D1Type::Integer(next_attempts),
                    D1Type::Text(&next_run_at),
                    D1Type::Text(&error_msg),
                    D1Type::Text(&now),
                    D1Type::Text(&job.id),
                ];
                let _ = db
                    .prepare(
                        "UPDATE background_jobs
                         SET status = ?1,
                             attempts = ?2,
                             next_run_at = ?3,
                             last_error = ?4,
                             updated_at = ?5
                         WHERE id = ?6",
                    )
                    .bind_refs(retry_bindings.iter())?
                    .run()
                    .await;
            }
        }
    }

    Ok(processed)
}

async fn handle_job(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
    job_type: &str,
    payload_json: &str,
) -> Result<()> {
    match job_type {
        JOB_CARD_UNFURL => handle_card_unfurl(db, payload_json).await,
        JOB_RESOLVE_IN_REPLY_TO => handle_resolve_in_reply_to(db, config, payload_json).await,
        JOB_REMOTE_CONTEXT_FETCH => {
            handle_remote_context_fetch(db, config, env, payload_json).await
        }
        JOB_REMOTE_STATUS_NOTIFY => {
            handle_remote_status_notify(db, config, env, payload_json).await
        }
        other => Err(Error::RustError(format!("unknown job type: {other}"))),
    }
}

// ─── card_unfurl ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CardUnfurlPayload {
    status_kind: String,
    status_id: String,
}

async fn handle_card_unfurl(db: &D1Database, payload_json: &str) -> Result<()> {
    let payload: CardUnfurlPayload = serde_json::from_str(payload_json)
        .map_err(|e| Error::RustError(format!("invalid card_unfurl payload: {e}")))?;

    match payload.status_kind.as_str() {
        "local" => {
            let Some(status) = find_status_by_id(db, &payload.status_id).await? else {
                return Ok(());
            };
            let Some(mut card) = build_status_card_value(&status.text) else {
                return Ok(());
            };
            enrich_card_with_remote_preview(&mut card).await?;
            let card_json =
                serde_json::to_string(&card).map_err(|e| Error::RustError(e.to_string()))?;
            let bindings = [D1Type::Text(&card_json), D1Type::Text(&status.id)];
            db.prepare(
                "UPDATE statuses SET card_json = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
            )
            .bind_refs(bindings.iter())?
            .run()
            .await?;
        }
        "remote" => {
            let Some(status) = find_remote_status_by_id(db, &payload.status_id).await? else {
                return Ok(());
            };
            let attachments =
                crate::find_remote_status_attachments_by_status_id(db, &status.id).await?;
            let text = status.plain_text();
            let Some(mut card) = build_remote_status_card_value(&text, &attachments) else {
                return Ok(());
            };
            enrich_card_with_remote_preview(&mut card).await?;
            let card_json =
                serde_json::to_string(&card).map_err(|e| Error::RustError(e.to_string()))?;
            let bindings = [D1Type::Text(&card_json), D1Type::Text(&status.id)];
            db.prepare("UPDATE remote_statuses SET card_json = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2")
                .bind_refs(bindings.iter())?
                .run()
                .await?;
        }
        other => {
            return Err(Error::RustError(format!(
                "invalid status_kind in card_unfurl: {other}"
            )));
        }
    }
    Ok(())
}

// ─── resolve_in_reply_to ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ResolveInReplyToPayload {
    remote_status_id: String,
}

async fn handle_resolve_in_reply_to(
    db: &D1Database,
    config: &AppConfig,
    payload_json: &str,
) -> Result<()> {
    let payload: ResolveInReplyToPayload = serde_json::from_str(payload_json)
        .map_err(|e| Error::RustError(format!("invalid resolve_in_reply_to payload: {e}")))?;

    let Some(status) = find_remote_status_by_id(db, &payload.remote_status_id).await? else {
        return Ok(());
    };
    if status.in_reply_to_id.is_some() {
        return Ok(());
    }
    let Some(ref uri) = status.in_reply_to_uri else {
        return Ok(());
    };

    let resolved_id = if let Some(remote) = find_remote_status_by_url_or_object_uri(db, uri).await?
    {
        Some(remote.id)
    } else if let Some(local) = crate::find_local_status_by_object_uri(db, config, uri).await? {
        Some(local.id)
    } else {
        None
    };

    if let Some(id) = resolved_id {
        let bindings = [D1Type::Text(&id), D1Type::Text(&payload.remote_status_id)];
        db.prepare(
            "UPDATE remote_statuses
             SET in_reply_to_id = ?1, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?2",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }
    Ok(())
}

// ─── remote_context_fetch ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RemoteContextFetchPayload {
    uri: String,
}

async fn handle_remote_context_fetch(
    db: &D1Database,
    config: &AppConfig,
    _env: Option<&Env>,
    payload_json: &str,
) -> Result<()> {
    let payload: RemoteContextFetchPayload = serde_json::from_str(payload_json)
        .map_err(|e| Error::RustError(format!("invalid remote_context_fetch payload: {e}")))?;

    // Skip if we already have it.
    if find_remote_status_by_url_or_object_uri(db, &payload.uri)
        .await?
        .is_some()
    {
        return Ok(());
    }

    // Best-effort: use the existing resolve path which handles auth checks.
    let _ = resolve_remote_status_by_url(db, config, &payload.uri, None).await;
    Ok(())
}

// ─── remote_status_notify ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RemoteStatusNotifyPayload {
    status_id: String,
    actor_uri: String,
    kind: String,
}

async fn handle_remote_status_notify(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
    payload_json: &str,
) -> Result<()> {
    let payload: RemoteStatusNotifyPayload =
        serde_json::from_str(payload_json).map_err(|error| {
            Error::RustError(format!("invalid remote_status_notify payload: {error}"))
        })?;
    crate::dispatch_remote_status_notifications(
        env,
        db,
        config,
        &payload.status_id,
        &payload.actor_uri,
        &payload.kind,
    )
    .await
}

// ─── Payload helpers used from call sites ────────────────────────────────────

pub(crate) fn card_unfurl_payload(status_kind: &str, status_id: &str) -> String {
    serde_json::json!({
        "status_kind": status_kind,
        "status_id": status_id,
    })
    .to_string()
}

pub(crate) fn resolve_in_reply_to_payload(remote_status_id: &str) -> String {
    serde_json::json!({ "remote_status_id": remote_status_id }).to_string()
}

pub(crate) fn remote_context_fetch_payload(uri: &str) -> String {
    serde_json::json!({ "uri": uri }).to_string()
}
