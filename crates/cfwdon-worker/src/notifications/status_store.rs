use super::{StatusRecord, StatusRow, statuses_from_records};
use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;

use crate::D1Database;
#[derive(Debug, Deserialize)]
pub(crate) struct RemoteStatusNotificationRow {
    pub(crate) id: String,
    pub(crate) actor_uri: String,
    pub(crate) object_uri: String,
    pub(crate) url: Option<String>,
    pub(crate) in_reply_to_uri: Option<String>,
    pub(crate) boost_of_uri: Option<String>,
    pub(crate) quote_of_uri: Option<String>,
    pub(crate) content_html: String,
    #[serde(default)]
    pub(crate) text_content: String,
    pub(crate) spoiler_text: String,
    pub(crate) visibility: String,
    pub(crate) sensitive: i32,
    pub(crate) language: Option<String>,
    #[serde(default = "crate::default_remote_quote_state")]
    pub(crate) quote_state: String,
    pub(crate) published_at: String,
    #[serde(default)]
    pub(crate) edited_at: Option<String>,
    #[serde(default)]
    pub(crate) card_json: Option<String>,
    #[serde(default)]
    pub(crate) federated_emojis_json: String,
    #[serde(default)]
    pub(crate) in_reply_to_id: Option<String>,
}

pub(crate) async fn list_local_status_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at
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

    crate::d1_results::<StatusRecord>(&result).and_then(statuses_from_records)
}

pub(crate) async fn list_remote_status_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<RemoteStatusNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT rs.id, rs.actor_uri, rs.object_uri, rs.url, rs.in_reply_to_uri, rs.boost_of_uri, rs.quote_of_uri, rs.content_html, rs.text_content, rs.spoiler_text, rs.visibility, rs.sensitive, rs.language, rs.quote_state, rs.published_at
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

    crate::d1_results::<RemoteStatusNotificationRow>(&result)
}
