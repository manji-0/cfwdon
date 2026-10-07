use super::{NotificationTimeWindow, StoredTimestampFormat};
use crate::content_helpers::extract_mentions_from_text;
use crate::db_utils::d1_results;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
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

pub(crate) async fn list_local_mention_notifications_for_account(
    db: &D1Database,
    viewer: &LocalAccount,
    config: &AppConfig,
    limit: u32,
    window: &NotificationTimeWindow,
) -> Result<Vec<MentionNotificationRow>> {
    let pattern = format!("%@{}%", viewer.username().to_ascii_lowercase());
    let bounds = window.sql_bounds(StoredTimestampFormat::Iso);
    let mut bindings = vec![
        D1Type::Text(viewer.id()),
        D1Type::Text(pattern.as_str()),
        D1Type::Integer(limit as i32),
    ];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT id, account_id, ap_id, in_reply_to_id, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at
             FROM statuses
             WHERE account_id != ?1
               AND lower(text_content) LIKE ?2{}
             ORDER BY created_at DESC
             LIMIT ?3",
            bounds.clause("created_at", 4)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    let mut rows = Vec::new();
    for row in d1_results::<MentionNotificationRow>(&result)? {
        if local_mention_row_targets_viewer(&row, viewer, config) {
            rows.push(row);
        }
    }

    Ok(rows)
}

fn local_mention_row_targets_viewer(
    row: &MentionNotificationRow,
    viewer: &LocalAccount,
    config: &AppConfig,
) -> bool {
    extract_mentions_from_text(&row.text_content, config)
        .into_iter()
        .any(|handle| handle.username == viewer.username())
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

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_domain::LocalAccountRecord;

    fn test_config() -> AppConfig {
        AppConfig::new(
            "example.com".to_owned(),
            "cfwdon".to_owned(),
            "test".to_owned(),
        )
    }

    fn test_viewer() -> LocalAccount {
        LocalAccount::from_record(LocalAccountRecord::test_fixture("acct-alice", "alice"))
    }

    #[test]
    fn local_mention_row_targets_viewer_uses_exact_mentions() {
        let viewer = test_viewer();
        let config = test_config();
        let row = MentionNotificationRow {
            id: "s1".to_owned(),
            account_id: "acct-bob".to_owned(),
            ap_id: None,
            in_reply_to_id: None,
            in_reply_to_account_id: None,
            quote_of_uri: None,
            content_html: String::new(),
            text_content: "hello @alice".to_owned(),
            spoiler_text: String::new(),
            visibility: "public".to_owned(),
            sensitive: 0,
            language: None,
            quote_state: "accepted".to_owned(),
            created_at: "2025-01-01T00:00:00Z".to_owned(),
        };
        assert!(local_mention_row_targets_viewer(&row, &viewer, &config));
    }
}
