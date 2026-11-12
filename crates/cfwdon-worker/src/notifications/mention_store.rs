use super::{NotificationTimeWindow, StoredTimestampFormat};
use crate::db_utils::d1_results;
use crate::tracked_d1::D1Database;
use cfwdon_domain::LocalAccount;
use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;
#[derive(Debug, Deserialize)]
pub(crate) struct MentionNotificationRow {
    pub(crate) id: String,
    pub(crate) account_id: String,
    pub(crate) ap_id: Option<String>,
    pub(crate) in_reply_to_id: Option<String>,
    #[serde(default)]
    pub(crate) in_reply_to_account_id: Option<String>,
    pub(crate) quote_of_uri: Option<String>,
    pub(crate) content_html: String,
    #[serde(rename = "text_content")]
    pub(crate) text_content: String,
    pub(crate) spoiler_text: String,
    pub(crate) visibility: String,
    pub(crate) sensitive: i32,
    pub(crate) language: Option<String>,
    #[serde(default = "crate::statuses::default_quote_state")]
    pub(crate) quote_state: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteMentionNotificationRow {
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
    #[serde(default = "crate::remote::default_remote_quote_state")]
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

/// Local statuses that mention the viewer, from the mention rows written when
/// each status was stored (`status_mentions_backfill` queues older statuses).
pub(crate) async fn list_local_mention_notifications_for_account(
    db: &D1Database,
    viewer: &LocalAccount,
    limit: u32,
    window: &NotificationTimeWindow,
) -> Result<Vec<MentionNotificationRow>> {
    let bounds = window.sql_bounds(StoredTimestampFormat::Iso);
    let mut bindings = vec![D1Type::Text(viewer.id()), D1Type::Integer(limit as i32)];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at
             FROM status_mentions m
             CROSS JOIN statuses s ON s.id = m.status_id
             WHERE m.account_id = ?1
               AND s.account_id != ?1{}
             ORDER BY m.created_at DESC
             LIMIT ?2",
            bounds.clause("m.created_at", 3)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<MentionNotificationRow>(&result)
}

/// Remote statuses that mention the viewer, from the mention rows written when
/// each status was stored (`scripts/backfill_remote_status_mentions.mjs`
/// fills them for statuses stored before those rows existed).
pub(crate) async fn list_remote_mention_notifications_for_account(
    db: &D1Database,
    viewer: &LocalAccount,
    limit: u32,
    window: &NotificationTimeWindow,
) -> Result<Vec<RemoteMentionNotificationRow>> {
    let bounds = window.sql_bounds(StoredTimestampFormat::Iso);
    let mut bindings = vec![D1Type::Text(viewer.id()), D1Type::Integer(limit as i32)];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT rs.id, rs.actor_uri, rs.object_uri, rs.url, rs.in_reply_to_uri, rs.boost_of_uri, rs.quote_of_uri, rs.content_html, rs.text_content, rs.spoiler_text, rs.visibility, rs.sensitive, rs.language, rs.quote_state, rs.published_at
             FROM remote_status_mentions m
             CROSS JOIN remote_statuses rs ON rs.id = m.status_id
             WHERE m.account_id = ?1{}
             ORDER BY m.published_at DESC
             LIMIT ?2",
            bounds.clause("m.published_at", 3)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<RemoteMentionNotificationRow>(&result)
}
