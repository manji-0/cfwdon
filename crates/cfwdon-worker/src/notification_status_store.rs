use crate::status_store::StatusRow;
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{D1Database, Result};

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteStatusNotificationRow {
    pub(crate) id: String,
    pub(crate) actor_uri: String,
    pub(crate) object_uri: String,
    pub(crate) url: Option<String>,
    pub(crate) in_reply_to_uri: Option<String>,
    pub(crate) content_html: String,
    pub(crate) spoiler_text: String,
    pub(crate) visibility: String,
    pub(crate) sensitive: i32,
    pub(crate) language: Option<String>,
    pub(crate) published_at: String,
}

pub(crate) async fn list_local_status_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.created_at
             FROM statuses s
             JOIN follows f
               ON f.target_account_id = s.account_id
              AND f.follower_account_id = ?1
              AND f.state = 'accepted'
              AND f.notify = 1
             WHERE s.account_id != ?1
               AND s.created_at >= f.updated_at
             ORDER BY s.created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<StatusRow>()
}

pub(crate) async fn list_remote_status_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<RemoteStatusNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT rs.id, rs.actor_uri, rs.object_uri, rs.url, rs.in_reply_to_uri, rs.content_html, rs.spoiler_text, rs.visibility, rs.sensitive, rs.language, rs.published_at
             FROM remote_statuses rs
             JOIN follows f
               ON f.target_actor_uri = rs.actor_uri
              AND f.follower_account_id = ?1
              AND f.state = 'accepted'
              AND f.notify = 1
             WHERE rs.published_at >= f.updated_at
             ORDER BY rs.published_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteStatusNotificationRow>()
}
