use super::RemoteStatusRecord;
use super::{RemoteStatusRow, remote_status_from_record};
use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;

use crate::D1Database;
#[derive(Debug, Deserialize)]
pub(crate) struct UpdateNotificationRow {
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
    pub(crate) remote_updated_at: String,
}

pub(crate) async fn list_update_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<UpdateNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT rs.id, rs.actor_uri, rs.object_uri, rs.url, rs.in_reply_to_uri, rs.boost_of_uri,
                    rs.quote_of_uri, rs.content_html, rs.text_content, rs.spoiler_text, rs.visibility, rs.sensitive,
                    rs.language, rs.quote_state, rs.published_at, rs.updated_at AS remote_updated_at
             FROM remote_statuses rs
             JOIN reblogs r
               ON r.remote_status_id = rs.id
             WHERE r.account_id = ?1
               AND rs.updated_at > r.updated_at
             ORDER BY rs.updated_at DESC, rs.id DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    crate::d1_results::<UpdateNotificationRow>(&result)
}

impl UpdateNotificationRow {
    pub(crate) fn as_remote_status_row(&self) -> Result<RemoteStatusRow> {
        remote_status_from_record(RemoteStatusRecord {
            id: self.id.clone(),
            actor_uri: self.actor_uri.clone(),
            object_uri: self.object_uri.clone(),
            url: self.url.clone(),
            in_reply_to_uri: self.in_reply_to_uri.clone(),
            boost_of_uri: self.boost_of_uri.clone(),
            quote_of_uri: self.quote_of_uri.clone(),
            content_html: self.content_html.clone(),
            text_content: self.text_content.clone(),
            spoiler_text: self.spoiler_text.clone(),
            visibility: self.visibility.clone(),
            sensitive: self.sensitive,
            language: self.language.clone(),
            quote_state: self.quote_state.clone(),
            published_at: self.published_at.clone(),
            edited_at: None,
            card_json: None,
            federated_emojis_json: "[]".to_owned(),
            in_reply_to_id: None,
        })
    }
}
