use super::{
    AppConfig, NotificationEntry, NotificationsQuery, clear_notifications_for_account,
    collect_visible_notifications, dismiss_notification_for_account,
};
use cfwdon_domain::LocalAccount;
use worker::Result;

use crate::D1Database;
pub(crate) async fn load_visible_notifications_for_account(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<Vec<NotificationEntry>> {
    collect_visible_notifications(db, config, viewer, query, per_type_limit).await
}

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
