use super::{NotificationEntry, notification_api_numeric_id, notification_api_numeric_id_string};
use crate::timestamp_to_mastodon_iso8601;

pub(crate) fn build_notifications_v2_document(entries: &[NotificationEntry]) -> serde_json::Value {
    let mut accounts = Vec::new();
    let mut account_ids = std::collections::HashSet::new();
    let mut statuses = Vec::new();
    let mut status_ids = std::collections::HashSet::new();
    let mut groups = Vec::new();

    for entry in entries {
        let account = entry.value.get("account").cloned();
        let account_id = account
            .as_ref()
            .and_then(|value| value.get("id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        if let (Some(account), Some(account_id)) = (account.clone(), account_id.clone())
            && account_ids.insert(account_id.clone())
        {
            accounts.push(account);
        }

        let status = entry.value.get("status").cloned();
        let status_id = status
            .as_ref()
            .and_then(|value| value.get("id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        if let (Some(status), Some(status_id)) = (status.clone(), status_id.clone())
            && status_ids.insert(status_id.clone())
        {
            statuses.push(status);
        }
        let collection = entry.value.get("collection").cloned();
        let api_notification_id = notification_api_numeric_id(entry);
        let api_notification_id_string = notification_api_numeric_id_string(entry);

        let mut group = serde_json::json!({
            "group_key": entry
                .value
                .get("group_key")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(entry.id.as_str()),
            "type": entry
                .value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "latest_page_notification_at": timestamp_to_mastodon_iso8601(&entry.created_at),
            "most_recent_notification_id": api_notification_id,
            "page_min_id": api_notification_id_string,
            "page_max_id": api_notification_id_string,
            "notifications_count": 1,
            "sample_account_ids": account_id.into_iter().collect::<Vec<_>>(),
            "status_id": status_id,
        });
        if let Some(collection) = collection {
            group["collection"] = collection;
        }
        groups.push(group);
    }

    serde_json::json!({
        "accounts": accounts,
        "statuses": statuses,
        "notification_groups": groups,
    })
}

pub(crate) fn build_notification_group_document(
    entries: &[&NotificationEntry],
) -> serde_json::Value {
    let group_entries = entries
        .iter()
        .map(|entry| NotificationEntry {
            id: entry.id.clone(),
            created_at: entry.created_at.clone(),
            value: entry.value.clone(),
        })
        .collect::<Vec<_>>();
    let document = build_notifications_v2_document(&group_entries);
    serde_json::json!({
        "accounts": document.get("accounts").cloned().unwrap_or_default(),
        "statuses": document.get("statuses").cloned().unwrap_or_default(),
        "notification_group": document
            .get("notification_groups")
            .and_then(serde_json::Value::as_array)
            .and_then(|groups| groups.first().cloned())
            .unwrap_or_default(),
    })
}
