use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;

use crate::D1Database;
#[derive(Debug, Deserialize)]
pub(crate) struct ReblogNotificationRow {
    pub(crate) account_id: String,
    pub(crate) status_id: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PollNotificationRow {
    pub(crate) poll_id: String,
    pub(crate) status_id: String,
    pub(crate) account_id: String,
    pub(crate) expires_at: String,
}

pub(crate) async fn list_reblog_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<ReblogNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT r.account_id, r.status_id, r.created_at
             FROM reblogs r
             JOIN statuses s
               ON s.id = r.status_id
             WHERE s.account_id = ?1
               AND r.account_id != ?1
               AND r.status_id IS NOT NULL
             ORDER BY r.created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<ReblogNotificationRow>()
}

pub(crate) async fn list_poll_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<PollNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT p.id AS poll_id,
                    p.status_id,
                    s.account_id,
                    p.expires_at
             FROM status_polls p
             JOIN statuses s
               ON s.id = p.status_id
             LEFT JOIN status_poll_votes v
               ON v.poll_id = p.id
              AND v.account_id = ?1
             WHERE datetime(replace(replace(p.expires_at, 'T', ' '), 'Z', '')) <= CURRENT_TIMESTAMP
               AND (s.account_id = ?1 OR v.account_id = ?1)
             GROUP BY p.id, p.status_id, s.account_id, p.expires_at
             ORDER BY p.expires_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<PollNotificationRow>()
}
