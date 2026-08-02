use super::notifications::{
    MastodonNotificationResponse, NotificationEntry, is_admin_account,
    notification_account_matches_filter, notification_type_allowed, push_notification_entry,
};
use super::{
    AppConfig, MastodonAccountResponse, NotificationsQuery, build_report_response,
    find_account_by_id, list_admin_report_notifications, list_admin_sign_up_notifications,
    load_account_stats,
};
use cfwdon_domain::LocalAccount;
use worker::Result;

use crate::D1Database;
pub(crate) async fn collect_admin_report_notifications_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !is_admin_account(config, viewer) || !notification_type_allowed(query, "admin.report") {
        return Ok(());
    }

    for report in list_admin_report_notifications(db, per_type_limit).await? {
        let Some(account) = find_account_by_id(db, &report.account_id).await? else {
            continue;
        };
        if !notification_account_matches_filter(query.account_id.as_deref(), account.id(), None) {
            continue;
        }
        let stats = load_account_stats(db, account.id()).await?;
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("admin-report-{}", report.id),
                notification_type: "admin.report".to_owned(),
                group_key: format!("admin-report-{}", report.id),
                created_at: report.created_at.clone(),
                account: MastodonAccountResponse::from_account_with_stats(&account, config, &stats),
                status: None,
                report: Some(serde_json::to_value(
                    build_report_response(db, config, &report).await?,
                )?),
            },
        );
    }

    Ok(())
}

pub(crate) async fn collect_admin_sign_up_notifications_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !is_admin_account(config, viewer) || !notification_type_allowed(query, "admin.sign_up") {
        return Ok(());
    }

    for account in list_admin_sign_up_notifications(db, viewer.id(), per_type_limit).await? {
        if !notification_account_matches_filter(query.account_id.as_deref(), account.id(), None) {
            continue;
        }
        let stats = load_account_stats(db, account.id()).await?;
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("admin-sign-up-{}", account.id()),
                notification_type: "admin.sign_up".to_owned(),
                group_key: format!("admin-sign-up-{}", account.id()),
                created_at: account.created_at().to_owned(),
                account: MastodonAccountResponse::from_account_with_stats(&account, config, &stats),
                status: None,
                report: None,
            },
        );
    }

    Ok(())
}
