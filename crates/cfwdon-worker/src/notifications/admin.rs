use super::{NotificationsQuery, list_admin_sign_up_notifications};
use crate::accounts::load_account_stats;
use crate::auth::find_account_by_id;
use crate::identity::remote_actor_uri_from_rest_id;
use crate::notifications::{
    MastodonNotificationResponse, NotificationEntry, is_admin_account,
    notification_account_matches_filter, notification_type_allowed, push_notification_entry,
};
use crate::reports::list_admin_report_notifications;
use crate::response::build_report_response;
use crate::responses::MastodonAccountResponse;
use crate::store::remote::find_remote_actor_by_actor_uri;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::Result;
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
        if !notification_account_matches_filter(
            query.account_id.as_deref(),
            &report.account_id,
            None,
        ) {
            continue;
        }
        let Some(account) = report_notification_account(db, config, &report.account_id).await?
        else {
            continue;
        };
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("admin-report-{}", report.id),
                notification_type: "admin.report".to_owned(),
                group_key: format!("admin-report-{}", report.id),
                created_at: report.created_at.clone(),
                account,
                status: None,
                report: Some(serde_json::to_value(
                    build_report_response(db, config, &report).await?,
                )?),
            },
        );
    }

    Ok(())
}

/// The reporter as the notification's `account`: a local account, or the
/// remote actor (an instance actor for an anonymous report) that sent a `Flag`.
async fn report_notification_account(
    db: &D1Database,
    config: &AppConfig,
    reporter_account_id: &str,
) -> Result<Option<MastodonAccountResponse>> {
    if let Some(actor_uri) = remote_actor_uri_from_rest_id(reporter_account_id) {
        return Ok(find_remote_actor_by_actor_uri(db, &actor_uri)
            .await?
            .map(|actor| MastodonAccountResponse::from_remote_actor(&actor)));
    }
    let Some(account) = find_account_by_id(db, reporter_account_id).await? else {
        return Ok(None);
    };
    let stats = load_account_stats(db, account.id()).await?;
    Ok(Some(MastodonAccountResponse::from_account_with_stats(
        &account, config, &stats,
    )))
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
