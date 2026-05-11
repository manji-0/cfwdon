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
    #[serde(default = "crate::default_quote_state")]
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
    pub(crate) spoiler_text: String,
    pub(crate) visibility: String,
    pub(crate) sensitive: i32,
    pub(crate) language: Option<String>,
    #[serde(default = "crate::default_remote_quote_state")]
    pub(crate) quote_state: String,
    pub(crate) published_at: String,
}

pub(crate) async fn list_local_mention_notifications_for_account(
    db: &D1Database,
    viewer: &LocalAccount,
    config: &AppConfig,
    limit: u32,
    min_created_at: Option<&str>,
) -> Result<Vec<MentionNotificationRow>> {
    let pattern = format!("%@{}%", viewer.username.to_ascii_lowercase());
    let result = if let Some(min_created_at) = min_created_at {
        let bindings = [
            D1Type::Text(viewer.id.as_str()),
            D1Type::Text(pattern.as_str()),
            D1Type::Text(min_created_at),
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at
             FROM statuses
             WHERE account_id != ?1
               AND lower(text_content) LIKE ?2
               AND created_at >= ?3
             ORDER BY created_at DESC
             LIMIT ?4",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    } else {
        let bindings = [
            D1Type::Text(viewer.id.as_str()),
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at
             FROM statuses
             WHERE account_id != ?1
               AND lower(text_content) LIKE ?2
             ORDER BY created_at DESC
             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    };

    let mut rows = Vec::new();
    for row in result.results::<MentionNotificationRow>()? {
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
        .any(|handle| handle.username == viewer.username)
}

pub(crate) async fn list_remote_mention_notifications_for_account(
    db: &D1Database,
    viewer: &LocalAccount,
    config: &AppConfig,
    limit: u32,
    min_published_at: Option<&str>,
) -> Result<Vec<RemoteMentionNotificationRow>> {
    let pattern = format!(
        "%@{}@{}%",
        viewer.username.to_ascii_lowercase(),
        instance_host(config)
    );
    let result = if let Some(min_published_at) = min_published_at {
        let bindings = [
            D1Type::Text(pattern.as_str()),
            D1Type::Text(min_published_at),
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
             FROM remote_statuses
             WHERE (lower(content_html) LIKE ?1
                OR lower(spoiler_text) LIKE ?1)
               AND published_at >= ?2
             ORDER BY published_at DESC
             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    } else {
        let bindings = [
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
             FROM remote_statuses
             WHERE lower(content_html) LIKE ?1
                OR lower(spoiler_text) LIKE ?1
             ORDER BY published_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    };

    // The SQL LIKE is only a cheap prefilter; HTML parsing keeps mention matching exact.
    let mut rows = Vec::new();
    for row in result.results::<RemoteMentionNotificationRow>()? {
        if remote_mention_row_targets_viewer(&row, viewer, config) {
            rows.push(row);
        }
    }

    Ok(rows)
}

fn remote_mention_row_targets_viewer(
    row: &RemoteMentionNotificationRow,
    viewer: &LocalAccount,
    config: &AppConfig,
) -> bool {
    let text_content = strip_html_tags(&row.content_html);
    extract_mentions_from_text(&text_content, config)
        .into_iter()
        .any(|handle| handle.username == viewer.username)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> AppConfig {
        AppConfig::new(
            "example.com".to_owned(),
            "cfwdon".to_owned(),
            "test".to_owned(),
        )
    }

    fn test_viewer() -> LocalAccount {
        LocalAccount {
            id: "acct-alice".to_owned(),
            username: "alice".to_owned(),
            access_email: "alice@example.com".to_owned(),
            display_name: "Alice".to_owned(),
            bio_html: String::new(),
            bio_text: String::new(),
            fields: Vec::new(),
            locked: false,
            bot: false,
            discoverable: true,
            default_post_visibility: "public".to_owned(),
            default_quote_policy: "public".to_owned(),
            default_sensitive: false,
            default_language: None,
            avatar_object_key: None,
            avatar_content_type: None,
            header_object_key: None,
            header_content_type: None,
            private_key_jwk: "{}".to_owned(),
            public_key_pem: "pem".to_owned(),
            created_at: "2025-01-01T00:00:00Z".to_owned(),
        }
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

    #[test]
    fn remote_mention_row_targets_viewer_strips_html() {
        let viewer = test_viewer();
        let config = test_config();
        let row = RemoteMentionNotificationRow {
            id: "rs1".to_owned(),
            actor_uri: "https://remote.example/users/bob".to_owned(),
            object_uri: "https://remote.example/statuses/1".to_owned(),
            url: None,
            in_reply_to_uri: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>hello <a>@alice@example.com</a></p>".to_owned(),
            spoiler_text: String::new(),
            visibility: "public".to_owned(),
            sensitive: 0,
            language: None,
            quote_state: "accepted".to_owned(),
            published_at: "2025-01-01T00:00:00Z".to_owned(),
        };
        assert!(remote_mention_row_targets_viewer(&row, &viewer, &config));
    }
}
