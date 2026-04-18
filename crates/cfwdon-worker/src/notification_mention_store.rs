use crate::content_helpers::{extract_mentions_from_text, strip_html_tags};
use crate::instance_identity::instance_host;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{D1Database, Result};

#[derive(Debug, Deserialize)]
pub(crate) struct MentionNotificationRow {
    pub(crate) id: String,
    pub(crate) account_id: String,
    pub(crate) ap_id: Option<String>,
    pub(crate) in_reply_to_id: Option<String>,
    pub(crate) quote_of_uri: Option<String>,
    pub(crate) content_html: String,
    #[serde(rename = "text_content")]
    pub(crate) text_content: String,
    pub(crate) spoiler_text: String,
    pub(crate) visibility: String,
    pub(crate) sensitive: i32,
    pub(crate) language: Option<String>,
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
    pub(crate) spoiler_text: String,
    pub(crate) visibility: String,
    pub(crate) sensitive: i32,
    pub(crate) language: Option<String>,
    pub(crate) published_at: String,
}

pub(crate) async fn list_local_mention_notifications_for_account(
    db: &D1Database,
    viewer: &LocalAccount,
    config: &AppConfig,
    limit: u32,
) -> Result<Vec<MentionNotificationRow>> {
    let pattern = format!("%@{}%", viewer.username.to_ascii_lowercase());
    let bindings = [
        D1Type::Text(viewer.id.as_str()),
        D1Type::Text(pattern.as_str()),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE account_id != ?1
               AND lower(text_content) LIKE ?2
             ORDER BY created_at DESC
             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    let mut rows = Vec::new();
    for row in result.results::<MentionNotificationRow>()? {
        if extract_mentions_from_text(&row.text_content, config)
            .into_iter()
            .any(|handle| handle.username == viewer.username)
        {
            rows.push(row);
        }
    }

    Ok(rows)
}

pub(crate) async fn list_remote_mention_notifications_for_account(
    db: &D1Database,
    viewer: &LocalAccount,
    config: &AppConfig,
    limit: u32,
) -> Result<Vec<RemoteMentionNotificationRow>> {
    let pattern = format!(
        "%@{}@{}%",
        viewer.username.to_ascii_lowercase(),
        instance_host(config)
    );
    let bindings = [
        D1Type::Text(pattern.as_str()),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, published_at
             FROM remote_statuses
             WHERE lower(content_html) LIKE ?1
                OR lower(spoiler_text) LIKE ?1
             ORDER BY published_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    let mut rows = Vec::new();
    for row in result.results::<RemoteMentionNotificationRow>()? {
        let text_content = strip_html_tags(&row.content_html);
        if extract_mentions_from_text(&text_content, config)
            .into_iter()
            .any(|handle| handle.username == viewer.username)
        {
            rows.push(row);
        }
    }

    Ok(rows)
}
