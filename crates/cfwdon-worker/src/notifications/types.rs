use super::{MastodonAccountResponse, MastodonStatusResponse};
use crate::timestamp_to_mastodon_iso8601;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct MastodonNotificationResponse {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) notification_type: String,
    pub(crate) group_key: String,
    pub(crate) created_at: String,
    pub(crate) account: MastodonAccountResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<MastodonStatusResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) report: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub(crate) struct NotificationEntry {
    pub(crate) id: String,
    pub(crate) created_at: String,
    pub(crate) value: serde_json::Value,
}

pub(crate) fn push_notification_entry(
    entries: &mut Vec<NotificationEntry>,
    mut notification: MastodonNotificationResponse,
) {
    notification.created_at = timestamp_to_mastodon_iso8601(&notification.created_at);
    let id = notification.id.clone();
    let created_at = notification.created_at.clone();
    entries.push(NotificationEntry {
        id,
        created_at,
        value: serde_json::to_value(notification).unwrap_or_default(),
    });
}
