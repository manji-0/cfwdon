use super::{clear_notifications_for_account, dismiss_notification_for_account};
use crate::tracked_d1::D1Database;
use worker::Result;
pub(crate) async fn dismiss_account_notification(
    db: &D1Database,
    account_id: &str,
    notification_id: &str,
) -> Result<()> {
    dismiss_notification_for_account(db, account_id, notification_id).await
}

pub(crate) async fn clear_account_notifications(db: &D1Database, account_id: &str) -> Result<()> {
    clear_notifications_for_account(db, account_id).await
}
