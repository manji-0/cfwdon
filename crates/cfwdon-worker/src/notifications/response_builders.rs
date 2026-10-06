use super::{NotificationEntry, notification_api_numeric_id, notification_api_numeric_id_string};
use crate::time_html::timestamp_to_mastodon_iso8601;

/// Types Mastodon folds into groups when the client does not pass `grouped_types[]`.
pub(crate) const GROUPABLE_NOTIFICATION_TYPES: &[&str] =
    &["favourite", "reblog", "follow", "admin.sign_up"];

/// Mastodon caps a group's time span at 12 hours (`MAXIMUM_GROUP_SPAN_HOURS`).
const NOTIFICATION_GROUP_SPAN_SECS: i64 = 12 * 60 * 60;
const MAX_SAMPLE_ACCOUNTS: usize = 8;

fn notification_str<'a>(entry: &'a NotificationEntry, key: &str) -> Option<&'a str> {
    entry.value.get(key).and_then(serde_json::Value::as_str)
}

fn notification_nested_id<'a>(entry: &'a NotificationEntry, key: &str) -> Option<&'a str> {
    entry
        .value
        .get(key)
        .and_then(|value| value.get("id"))
        .and_then(serde_json::Value::as_str)
}

fn notification_unix_time(entry: &NotificationEntry) -> i64 {
    time::OffsetDateTime::parse(
        &timestamp_to_mastodon_iso8601(&entry.created_at),
        &time::format_description::well_known::Rfc3339,
    )
    .map(|value| value.unix_timestamp())
    .unwrap_or_default()
}

/// Mastodon's `Notification#group_key`: favourites and reblogs of one status,
/// or follows / sign-ups, within one 12-hour bucket share a key; anything not
/// in `grouped_types` stays `ungrouped-<id>`.
pub(crate) fn notification_v2_group_key(
    entry: &NotificationEntry,
    grouped_types: &[String],
) -> String {
    let notification_type = notification_str(entry, "type").unwrap_or_default();
    let prefix = if !grouped_types.iter().any(|value| value == notification_type) {
        None
    } else {
        match notification_type {
            "favourite" | "reblog" => notification_nested_id(entry, "status")
                .map(|status_id| format!("{notification_type}-{status_id}")),
            "follow" | "admin.sign_up" => Some(notification_type.to_owned()),
            _ => None,
        }
    };
    match prefix {
        Some(prefix) => format!(
            "{prefix}-{}",
            notification_unix_time(entry).div_euclid(NOTIFICATION_GROUP_SPAN_SECS)
        ),
        None => format!("ungrouped-{}", notification_api_numeric_id(entry)),
    }
}

pub(crate) fn default_grouped_notification_types() -> Vec<String> {
    GROUPABLE_NOTIFICATION_TYPES
        .iter()
        .map(|value| (*value).to_owned())
        .collect()
}

/// Render newest-first `entries` as a grouped-notifications document.
pub(crate) fn build_notifications_v2_document(
    entries: &[NotificationEntry],
    grouped_types: &[String],
) -> serde_json::Value {
    let mut accounts = Vec::new();
    let mut account_ids = std::collections::HashSet::new();
    let mut statuses = Vec::new();
    let mut status_ids = std::collections::HashSet::new();
    let mut group_order = Vec::<String>::new();
    let mut members = std::collections::HashMap::<String, Vec<&NotificationEntry>>::new();

    for entry in entries {
        if let (Some(account), Some(account_id)) = (
            entry.value.get("account"),
            notification_nested_id(entry, "account"),
        ) && account_ids.insert(account_id.to_owned())
        {
            accounts.push(account.clone());
        }
        if let (Some(status), Some(status_id)) = (
            entry.value.get("status"),
            notification_nested_id(entry, "status"),
        ) && status_ids.insert(status_id.to_owned())
        {
            statuses.push(status.clone());
        }

        let key = notification_v2_group_key(entry, grouped_types);
        let group = members.entry(key.clone()).or_default();
        if group.is_empty() {
            group_order.push(key);
        }
        group.push(entry);
    }

    let groups = group_order
        .into_iter()
        .filter_map(|key| {
            let group = members.remove(&key)?;
            Some(notification_group_value(&key, &group))
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "accounts": accounts,
        "statuses": statuses,
        "notification_groups": groups,
    })
}

fn notification_group_value(key: &str, group: &[&NotificationEntry]) -> serde_json::Value {
    let newest = group[0];
    let oldest = group[group.len() - 1];
    let mut sample_account_ids = Vec::new();
    for entry in group {
        if let Some(account_id) = notification_nested_id(entry, "account")
            && !sample_account_ids.iter().any(|id| id == account_id)
            && sample_account_ids.len() < MAX_SAMPLE_ACCOUNTS
        {
            sample_account_ids.push(account_id.to_owned());
        }
    }
    let mut value = serde_json::json!({
        "group_key": key,
        "type": notification_str(newest, "type").unwrap_or_default(),
        "latest_page_notification_at": timestamp_to_mastodon_iso8601(&newest.created_at),
        "most_recent_notification_id": notification_api_numeric_id(newest),
        "page_min_id": notification_api_numeric_id_string(oldest),
        "page_max_id": notification_api_numeric_id_string(newest),
        "notifications_count": group.len(),
        "sample_account_ids": sample_account_ids,
        "status_id": notification_nested_id(newest, "status"),
    });
    if let Some(collection) = newest.value.get("collection") {
        value["collection"] = collection.clone();
    }
    value
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
    let document =
        build_notifications_v2_document(&group_entries, &default_grouped_notification_types());
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
