use super::{NotificationTimeWindow, StoredTimestampFormat};
use crate::db_utils::d1_results;
use crate::remote::remote_statuses_from_records;
use crate::statuses::statuses_from_records;
use crate::tracked_d1::D1Database;
use cfwdon_domain::{LocalStatus, LocalStatusRecord, RemoteStatus, RemoteStatusRecord};
use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;
/// One of the viewer's statuses whose quoted remote status was edited.
#[derive(Debug, Deserialize)]
pub(crate) struct QuotedUpdateNotificationRow {
    pub(crate) id: String,
    pub(crate) remote_actor_uri: String,
    pub(crate) remote_updated_at: String,
}

pub(crate) async fn list_local_quote_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    window: &NotificationTimeWindow,
) -> Result<Vec<LocalStatus>> {
    let bounds = window.sql_bounds(StoredTimestampFormat::Iso);
    let mut bindings = vec![D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at
             FROM statuses s
             JOIN statuses target
               ON target.ap_id = s.quote_of_uri
             WHERE target.account_id = ?1
               AND s.account_id != ?1
               AND s.quote_state = 'accepted'{}
             ORDER BY s.created_at DESC
             LIMIT ?2",
            bounds.clause("s.created_at", 3)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<LocalStatusRecord>(&result).and_then(statuses_from_records)
}

pub(crate) async fn list_remote_quote_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    window: &NotificationTimeWindow,
) -> Result<Vec<RemoteStatus>> {
    let bounds = window.sql_bounds(StoredTimestampFormat::Iso);
    let mut bindings = vec![D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT rs.id, rs.actor_uri, rs.object_uri, rs.url, rs.in_reply_to_uri, rs.boost_of_uri, rs.quote_of_uri, rs.content_html, rs.text_content, rs.spoiler_text, rs.visibility, rs.sensitive, rs.language, rs.quote_state, rs.published_at
             FROM remote_statuses rs
             JOIN statuses target
               ON target.ap_id = rs.quote_of_uri
             WHERE target.account_id = ?1
               AND rs.quote_state = 'accepted'{}
             ORDER BY rs.published_at DESC, rs.id DESC
             LIMIT ?2",
            bounds.clause("rs.published_at", 3)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<RemoteStatusRecord>(&result).and_then(remote_statuses_from_records)
}

pub(crate) async fn list_quoted_update_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    window: &NotificationTimeWindow,
) -> Result<Vec<QuotedUpdateNotificationRow>> {
    let bounds = window.sql_bounds(StoredTimestampFormat::Iso);
    let mut bindings = vec![D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT s.id, rs.actor_uri AS remote_actor_uri, rs.edited_at AS remote_updated_at
             FROM statuses s
             JOIN remote_statuses rs
               ON rs.object_uri = s.quote_of_uri
             WHERE s.account_id = ?1
               AND s.quote_state != 'revoked'
               AND rs.edited_at IS NOT NULL
               AND rs.edited_at > s.created_at{}
             ORDER BY rs.edited_at DESC, s.created_at DESC
             LIMIT ?2",
            bounds.clause("rs.edited_at", 3)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<QuotedUpdateNotificationRow>(&result)
}
