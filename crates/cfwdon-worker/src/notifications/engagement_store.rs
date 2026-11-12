use super::{NotificationTimeWindow, StoredTimestampFormat};
use crate::db_utils::d1_results;
use crate::tracked_d1::D1Database;
use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;
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
    window: &NotificationTimeWindow,
) -> Result<Vec<ReblogNotificationRow>> {
    let bounds = window.sql_bounds(StoredTimestampFormat::Sqlite);
    let mut bindings = vec![D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT r.account_id, r.status_id, r.created_at
             FROM reblogs r
             WHERE r.notified_account_id = ?1{}
             ORDER BY r.created_at DESC
             LIMIT ?2",
            bounds.clause("r.created_at", 3)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<ReblogNotificationRow>(&result)
}

pub(crate) async fn list_poll_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    window: &NotificationTimeWindow,
) -> Result<Vec<PollNotificationRow>> {
    let bounds = window.sql_bounds(StoredTimestampFormat::Iso);
    let mut bindings = vec![D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT p.id AS poll_id,
                    p.status_id,
                    s.account_id,
                    p.expires_at
             FROM status_polls p
             JOIN statuses s
               ON s.id = p.status_id
             WHERE p.id IN (
                     SELECT id FROM status_polls WHERE account_id = ?1
                     UNION
                     SELECT poll_id FROM status_poll_votes WHERE account_id = ?1
                   )
               AND datetime(replace(replace(p.expires_at, 'T', ' '), 'Z', '')) <= CURRENT_TIMESTAMP{}
             ORDER BY p.expires_at DESC
             LIMIT ?2",
            bounds.clause("p.expires_at", 3)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<PollNotificationRow>(&result)
}
