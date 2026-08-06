use super::now_iso_string;
use serde::Deserialize;
use std::collections::HashSet;
use worker::Result;
use worker::d1::D1Type;

use crate::D1Database;
#[derive(Debug, Deserialize)]
pub(crate) struct NotificationDismissalRow {
    pub(crate) notification_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NotificationClearMarkerRow {
    pub(crate) cleared_at: String,
}

pub(crate) async fn load_dismissed_notification_ids(
    db: &D1Database,
    account_id: &str,
) -> Result<HashSet<String>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT notification_id
             FROM notification_dismissals
             WHERE account_id = ?1",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;

    Ok(crate::d1_results::<NotificationDismissalRow>(&result)?
        .into_iter()
        .map(|row| row.notification_id)
        .collect())
}

pub(crate) async fn load_notification_clear_marker(
    db: &D1Database,
    account_id: &str,
) -> Result<Option<String>> {
    let account_id = D1Type::Text(account_id);
    Ok(db
        .prepare(
            "SELECT cleared_at
             FROM notification_clear_markers
             WHERE account_id = ?1
             LIMIT 1",
        )
        .bind_refs(&account_id)?
        .first::<NotificationClearMarkerRow>(None)
        .await?
        .map(|row| row.cleared_at))
}

pub(crate) async fn dismiss_notification_for_account(
    db: &D1Database,
    account_id: &str,
    notification_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(notification_id)];
    db.prepare(
        "INSERT INTO notification_dismissals (account_id, notification_id)
         VALUES (?1, ?2)
         ON CONFLICT(account_id, notification_id) DO NOTHING",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn clear_notifications_for_account(
    db: &D1Database,
    account_id: &str,
) -> Result<()> {
    let cleared_at = now_iso_string()?;
    let bindings = [D1Type::Text(account_id), D1Type::Text(cleared_at.as_str())];
    db.prepare(
        "INSERT INTO notification_clear_markers (account_id, cleared_at)
         VALUES (?1, ?2)
         ON CONFLICT(account_id) DO UPDATE SET
             cleared_at = excluded.cleared_at",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}
