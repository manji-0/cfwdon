use super::{
    RemoteStatusRecord, RemoteStatusRow, StatusRecord, StatusRow, remote_statuses_from_records,
    statuses_from_records,
};
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{D1Database, Result};

#[derive(Debug, Deserialize)]
pub(crate) struct QuotedUpdateNotificationRow {
    pub(crate) id: String,
    pub(crate) account_id: String,
    pub(crate) ap_id: Option<String>,
    pub(crate) in_reply_to_id: Option<String>,
    pub(crate) boost_of_uri: Option<String>,
    pub(crate) quote_of_uri: Option<String>,
    pub(crate) content_html: String,
    #[serde(rename = "text_content")]
    pub(crate) text_content: String,
    pub(crate) spoiler_text: String,
    pub(crate) visibility: String,
    pub(crate) sensitive: i32,
    pub(crate) language: Option<String>,
    #[serde(default = "crate::default_quote_state")]
    pub(crate) quote_state: String,
    pub(crate) created_at: String,
    pub(crate) remote_actor_uri: String,
    pub(crate) remote_updated_at: String,
}

pub(crate) async fn list_local_quote_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at
             FROM statuses s
             JOIN statuses target
               ON target.ap_id = s.quote_of_uri
             WHERE target.account_id = ?1
               AND s.account_id != ?1
               AND s.quote_state = 'accepted'
             ORDER BY s.created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result
        .results::<StatusRecord>()
        .and_then(statuses_from_records)
}

pub(crate) async fn list_remote_quote_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<RemoteStatusRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT rs.id, rs.actor_uri, rs.object_uri, rs.url, rs.in_reply_to_uri, rs.boost_of_uri, rs.quote_of_uri, rs.content_html, rs.spoiler_text, rs.visibility, rs.sensitive, rs.language, rs.quote_state, rs.published_at
             FROM remote_statuses rs
             JOIN statuses target
               ON target.ap_id = rs.quote_of_uri
             WHERE target.account_id = ?1
               AND rs.quote_state = 'accepted'
             ORDER BY rs.published_at DESC, rs.id DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result
        .results::<RemoteStatusRecord>()
        .and_then(remote_statuses_from_records)
}

pub(crate) async fn list_quoted_update_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<QuotedUpdateNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at,
                    rs.actor_uri AS remote_actor_uri, rs.updated_at AS remote_updated_at
             FROM statuses s
             JOIN remote_statuses rs
               ON rs.object_uri = s.quote_of_uri
             WHERE s.account_id = ?1
               AND s.quote_state != 'revoked'
               AND rs.updated_at > rs.created_at
             ORDER BY rs.updated_at DESC, s.created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<QuotedUpdateNotificationRow>()
}
