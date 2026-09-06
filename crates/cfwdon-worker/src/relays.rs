use crate::{
    AppConfig, D1Database, Error, LocalAccount, RemoteActorProfile, Result, actor_url,
    build_relay_follow_activity, build_relay_undo_follow_activity,
    delivery_inbox_blocked_by_domains, enqueue_targeted_outbox_activity, ensure_account_keys,
    generate_entity_id, list_instance_domain_block_domains, parse_remote_http_url,
};
use serde::{Deserialize, Serialize};
use url::Url;
use worker::d1::D1Type;

pub(crate) const RELAY_STATE_IDLE: &str = "idle";
pub(crate) const RELAY_STATE_PENDING: &str = "pending";
pub(crate) const RELAY_STATE_ACCEPTED: &str = "accepted";
pub(crate) const RELAY_STATE_REJECTED: &str = "rejected";

pub(crate) const PUBLIC_REMOTE_RETENTION_DAYS: i64 = 7;
pub(crate) const PUBLIC_REMOTE_PURGE_BATCH_SIZE: u32 = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FederationRelayRow {
    pub(crate) id: String,
    pub(crate) inbox_url: String,
    pub(crate) actor_uri: Option<String>,
    pub(crate) follow_activity_id: Option<String>,
    pub(crate) signing_account_id: String,
    pub(crate) state: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

fn normalize_relay_inbox_url(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::RustError(
            "relay inbox URL must not be empty".to_owned(),
        ));
    }
    let parsed = parse_remote_http_url(trimmed)?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err(Error::RustError(
            "relay inbox URL must use http or https".to_owned(),
        ));
    }
    Ok(parsed.to_string())
}

fn relay_row_from_value(value: serde_json::Value) -> Option<FederationRelayRow> {
    Some(FederationRelayRow {
        id: value.get("id")?.as_str()?.to_owned(),
        inbox_url: value.get("inbox_url")?.as_str()?.to_owned(),
        actor_uri: value
            .get("actor_uri")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        follow_activity_id: value
            .get("follow_activity_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        signing_account_id: value.get("signing_account_id")?.as_str()?.to_owned(),
        state: value.get("state")?.as_str()?.to_owned(),
        created_at: value.get("created_at")?.as_str()?.to_owned(),
        updated_at: value.get("updated_at")?.as_str()?.to_owned(),
    })
}

pub(crate) async fn list_federation_relays(db: &D1Database) -> Result<Vec<FederationRelayRow>> {
    let result = db
        .prepare(
            "SELECT id, inbox_url, actor_uri, follow_activity_id, signing_account_id, state, created_at, updated_at
             FROM federation_relays
             ORDER BY created_at DESC",
        )
        .all()
        .await?;
    Ok(crate::d1_results::<serde_json::Value>(&result)?
        .into_iter()
        .filter_map(relay_row_from_value)
        .collect())
}

pub(crate) async fn find_federation_relay_by_id(
    db: &D1Database,
    relay_id: &str,
) -> Result<Option<FederationRelayRow>> {
    let row = db
        .prepare(
            "SELECT id, inbox_url, actor_uri, follow_activity_id, signing_account_id, state, created_at, updated_at
             FROM federation_relays
             WHERE id = ?1",
        )
        .bind_refs(&[D1Type::Text(relay_id)])?
        .first::<serde_json::Value>(None)
        .await?;
    Ok(row.and_then(relay_row_from_value))
}

pub(crate) async fn list_enabled_relay_inbox_urls(db: &D1Database) -> Result<Vec<String>> {
    let blocked_domains = list_instance_domain_block_domains(db).await?;
    let result = db
        .prepare(
            "SELECT inbox_url
             FROM federation_relays
             WHERE state = ?1
             ORDER BY created_at ASC",
        )
        .bind_refs(&[D1Type::Text(RELAY_STATE_ACCEPTED)])?
        .all()
        .await?;
    Ok(crate::d1_results::<serde_json::Value>(&result)?
        .into_iter()
        .filter_map(|row| {
            row.get("inbox_url")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .filter(|inbox| !delivery_inbox_blocked_by_domains(inbox, &blocked_domains))
        .collect())
}

pub(crate) async fn relay_delivery_is_enabled(
    db: &D1Database,
    delivery_actor: &RemoteActorProfile,
) -> Result<bool> {
    let actor_uri = delivery_actor.actor_uri.as_str();
    let actor_host = Url::parse(actor_uri)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase));
    let rows = db
        .prepare(
            "SELECT inbox_url, actor_uri
             FROM federation_relays
             WHERE state = ?1",
        )
        .bind_refs(&[D1Type::Text(RELAY_STATE_ACCEPTED)])?
        .all()
        .await
        .and_then(|__d1| crate::d1_results::<serde_json::Value>(&__d1))?;
    for row in rows {
        if row
            .get("actor_uri")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| value == actor_uri)
        {
            return Ok(true);
        }
        let Some(inbox_url) = row.get("inbox_url").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Ok(inbox) = Url::parse(inbox_url) else {
            continue;
        };
        let Some(inbox_host) = inbox.host_str().map(str::to_ascii_lowercase) else {
            continue;
        };
        if actor_host.as_deref() == Some(inbox_host.as_str()) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) async fn create_and_enable_federation_relay(
    db: &D1Database,
    config: &AppConfig,
    admin: &LocalAccount,
    inbox_url: &str,
) -> Result<FederationRelayRow> {
    let inbox_url = normalize_relay_inbox_url(inbox_url)?;
    let admin = ensure_account_keys(db, config, admin.clone()).await?;
    let relay_id = generate_entity_id(16)?;
    let follow_activity_id = format!(
        "{}/relay-follows/{}",
        actor_url(config, admin.username()),
        generate_entity_id(12)?
    );
    let payload = build_relay_follow_activity(config, &admin, &follow_activity_id)?;
    db.prepare(
        "INSERT INTO federation_relays (
            id,
            inbox_url,
            follow_activity_id,
            signing_account_id,
            state,
            created_at,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .bind_refs(&[
        D1Type::Text(&relay_id),
        D1Type::Text(&inbox_url),
        D1Type::Text(&follow_activity_id),
        D1Type::Text(admin.id()),
        D1Type::Text(RELAY_STATE_PENDING),
    ])?
    .run()
    .await?;
    enqueue_targeted_outbox_activity(db, admin.id(), None, &payload, &[inbox_url]).await?;
    find_federation_relay_by_id(db, &relay_id)
        .await?
        .ok_or_else(|| Error::RustError("created relay could not be reloaded".to_owned()))
}

pub(crate) async fn disable_federation_relay(
    db: &D1Database,
    config: &AppConfig,
    relay_id: &str,
) -> Result<bool> {
    let Some(relay) = find_federation_relay_by_id(db, relay_id).await? else {
        return Ok(false);
    };
    if relay.state == RELAY_STATE_IDLE {
        return Ok(true);
    }
    let Some(account) = crate::find_account_by_id(db, &relay.signing_account_id).await? else {
        return Err(Error::RustError(
            "relay signing account is missing".to_owned(),
        ));
    };
    let account = ensure_account_keys(db, config, account).await?;
    if let Some(follow_activity_id) = relay.follow_activity_id.as_deref() {
        let payload = build_relay_undo_follow_activity(config, &account, follow_activity_id)?;
        enqueue_targeted_outbox_activity(
            db,
            account.id(),
            None,
            &payload,
            std::slice::from_ref(&relay.inbox_url),
        )
        .await?;
    }
    db.prepare(
        "UPDATE federation_relays
         SET state = ?1,
             follow_activity_id = NULL,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2",
    )
    .bind_refs(&[D1Type::Text(RELAY_STATE_IDLE), D1Type::Text(relay_id)])?
    .run()
    .await?;
    Ok(true)
}

pub(crate) async fn delete_federation_relay(
    db: &D1Database,
    config: &AppConfig,
    relay_id: &str,
) -> Result<bool> {
    let _ = disable_federation_relay(db, config, relay_id).await?;
    let result = db
        .prepare("DELETE FROM federation_relays WHERE id = ?1")
        .bind_refs(&[D1Type::Text(relay_id)])?
        .run()
        .await?;
    Ok(result.meta()?.and_then(|meta| meta.changes).unwrap_or(0) > 0)
}

pub(crate) async fn mark_federation_relay_accepted(
    db: &D1Database,
    follow_activity_id: &str,
    relay_actor_uri: &str,
) -> Result<bool> {
    let result = db
        .prepare(
            "UPDATE federation_relays
             SET state = ?1,
                 actor_uri = ?2,
                 updated_at = CURRENT_TIMESTAMP
             WHERE follow_activity_id = ?3",
        )
        .bind_refs(&[
            D1Type::Text(RELAY_STATE_ACCEPTED),
            D1Type::Text(relay_actor_uri),
            D1Type::Text(follow_activity_id),
        ])?
        .run()
        .await?;
    Ok(result.meta()?.and_then(|meta| meta.changes).unwrap_or(0) > 0)
}

pub(crate) async fn mark_federation_relay_rejected(
    db: &D1Database,
    follow_activity_id: &str,
) -> Result<bool> {
    let result = db
        .prepare(
            "UPDATE federation_relays
             SET state = ?1,
                 updated_at = CURRENT_TIMESTAMP
             WHERE follow_activity_id = ?2",
        )
        .bind_refs(&[
            D1Type::Text(RELAY_STATE_REJECTED),
            D1Type::Text(follow_activity_id),
        ])?
        .run()
        .await?;
    Ok(result.meta()?.and_then(|meta| meta.changes).unwrap_or(0) > 0)
}

fn stale_public_remote_status_purge_sql() -> &'static str {
    // One equality per NOT EXISTS so SQLite can use boost_of_uri / quote_of_uri
    // / in_reply_to_uri indexes. A 6-way OR here table-scans remote_statuses
    // once per candidate row.
    "DELETE FROM remote_statuses
     WHERE id IN (
        SELECT rs.id
        FROM remote_statuses rs
        WHERE rs.visibility = 'public'
          AND rs.published_at < datetime(CURRENT_TIMESTAMP, ?1)
          AND NOT EXISTS (
            SELECT 1
            FROM follows f
            WHERE f.target_actor_uri = rs.actor_uri
              AND f.state = 'accepted'
          )
          AND NOT EXISTS (
            SELECT 1
            FROM followers fl
            WHERE fl.actor_uri = rs.actor_uri
          )
          AND NOT EXISTS (
            SELECT 1
            FROM remote_statuses ref
            WHERE ref.id != rs.id
              AND ref.boost_of_uri = rs.object_uri
          )
          AND NOT EXISTS (
            SELECT 1
            FROM remote_statuses ref
            WHERE ref.id != rs.id
              AND ref.boost_of_uri = rs.url
          )
          AND NOT EXISTS (
            SELECT 1
            FROM remote_statuses ref
            WHERE ref.id != rs.id
              AND ref.quote_of_uri = rs.object_uri
          )
          AND NOT EXISTS (
            SELECT 1
            FROM remote_statuses ref
            WHERE ref.id != rs.id
              AND ref.quote_of_uri = rs.url
          )
          AND NOT EXISTS (
            SELECT 1
            FROM remote_statuses ref
            WHERE ref.id != rs.id
              AND ref.in_reply_to_uri = rs.object_uri
          )
          AND NOT EXISTS (
            SELECT 1
            FROM remote_statuses ref
            WHERE ref.id != rs.id
              AND ref.in_reply_to_uri = rs.url
          )
        ORDER BY rs.published_at ASC
        LIMIT ?2
     )"
}

pub(crate) async fn purge_stale_public_remote_content(db: &D1Database) -> Result<u32> {
    let cutoff_modifier = format!("-{PUBLIC_REMOTE_RETENTION_DAYS} days");
    let limit = i32::try_from(PUBLIC_REMOTE_PURGE_BATCH_SIZE).unwrap_or(500);
    // Keep statuses still needed as boost/quote/reply targets even when the
    // author is not followed — otherwise timeline reblogs render empty bodies.
    let status_result = db
        .prepare(stale_public_remote_status_purge_sql())
        .bind_refs(&[D1Type::Text(&cutoff_modifier), D1Type::Integer(limit)])?
        .run()
        .await?;
    let actor_result = db
        .prepare(
            "DELETE FROM remote_actors
             WHERE actor_uri IN (
                SELECT ra.actor_uri
                FROM remote_actors ra
                LEFT JOIN remote_statuses rs ON rs.actor_uri = ra.actor_uri
                WHERE rs.id IS NULL
                  AND NOT EXISTS (
                    SELECT 1
                    FROM follows f
                    WHERE f.target_actor_uri = ra.actor_uri
                      AND f.state = 'accepted'
                  )
                  AND NOT EXISTS (
                    SELECT 1
                    FROM followers fl
                    WHERE fl.actor_uri = ra.actor_uri
                  )
                LIMIT ?1
             )",
        )
        .bind_refs(&[D1Type::Integer(limit)])?
        .run()
        .await?;
    Ok(status_result
        .meta()?
        .and_then(|meta| meta.changes)
        .unwrap_or(0)
        .saturating_add(
            actor_result
                .meta()?
                .and_then(|meta| meta.changes)
                .unwrap_or(0),
        ) as u32)
}

pub(crate) fn outbox_payload_is_public_relay_candidate(payload_json: &str) -> bool {
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(payload_json) else {
        return false;
    };
    let activity_type = payload
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if !matches!(activity_type, "Create" | "Update" | "Delete") {
        return false;
    }
    if activity_type == "Delete" {
        return true;
    }
    payload
        .get("object")
        .is_some_and(crate::note_targets_public)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbox_payload_is_public_relay_candidate_accepts_public_create() {
        let payload = serde_json::json!({
            "type": "Create",
            "object": {
                "type": "Note",
                "to": ["https://www.w3.org/ns/activitystreams#Public"]
            }
        });
        assert!(outbox_payload_is_public_relay_candidate(
            &serde_json::to_string(&payload).unwrap()
        ));
    }

    #[test]
    fn stale_public_remote_status_purge_sql_avoids_or_on_reference_columns() {
        let sql = stale_public_remote_status_purge_sql();
        assert!(!sql.contains(" OR "));
        assert!(sql.contains("ref.boost_of_uri = rs.object_uri"));
        assert!(sql.contains("ref.boost_of_uri = rs.url"));
        assert!(sql.contains("ref.quote_of_uri = rs.object_uri"));
        assert!(sql.contains("ref.quote_of_uri = rs.url"));
        assert!(sql.contains("ref.in_reply_to_uri = rs.object_uri"));
        assert!(sql.contains("ref.in_reply_to_uri = rs.url"));
        assert_eq!(sql.matches("NOT EXISTS").count(), 8);
    }

    #[test]
    fn outbox_payload_is_public_relay_candidate_rejects_direct_create() {
        let payload = serde_json::json!({
            "type": "Create",
            "object": {
                "type": "Note",
                "to": ["https://remote.example/users/alice"]
            }
        });
        assert!(!outbox_payload_is_public_relay_candidate(
            &serde_json::to_string(&payload).unwrap()
        ));
    }
}
