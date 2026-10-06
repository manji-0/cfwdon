use crate::notifications::{
    NotificationEntry, NotificationsQuery, build_notifications_v2_document,
    count_unread_notification_entries, default_grouped_notification_types,
    filter_notification_entries_by_query, is_admin_account, is_admin_authorized,
    notification_api_numeric_id, notification_timestamp_sort_token, notification_v1_value,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::{LocalAccount, LocalAccountRecord};

#[test]
fn build_notifications_v2_document_collects_accounts_statuses_and_groups() {
    let entries = vec![
        NotificationEntry {
            id: "mention-1".to_owned(),
            created_at: "2026-04-19T00:00:00Z".to_owned(),
            value: serde_json::json!({
                "id": "mention-1",
                "type": "mention",
                "group_key": "mention-1",
                "account": {"id": "alice@remote.example", "acct": "alice@remote.example"},
                "status": {"id": "status-1", "content": "<p>hello</p>"}
            }),
        },
        NotificationEntry {
            id: "follow-1".to_owned(),
            created_at: "2026-04-19T00:01:00Z".to_owned(),
            value: serde_json::json!({
                "id": "follow-1",
                "type": "follow",
                "group_key": "follow-1",
                "account": {"id": "alice@remote.example", "acct": "alice@remote.example"}
            }),
        },
    ];

    let document = build_notifications_v2_document(&entries, &default_grouped_notification_types());
    assert_eq!(document["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(document["statuses"].as_array().unwrap().len(), 1);
    assert_eq!(document["notification_groups"].as_array().unwrap().len(), 2);
    assert_eq!(document["notification_groups"][0]["type"], "mention");
    assert_eq!(document["notification_groups"][0]["notifications_count"], 1);
    assert_eq!(
        document["notification_groups"][0]["sample_account_ids"],
        serde_json::json!(["alice@remote.example"])
    );
    assert_eq!(document["notification_groups"][0]["status_id"], "status-1");
    assert_eq!(
        document["notification_groups"][1]["status_id"],
        serde_json::Value::Null
    );

    let mention_group = &document["notification_groups"][0];
    let mention_entry = &entries[0];
    let mention_api_id = notification_api_numeric_id(mention_entry);
    assert_eq!(mention_group["most_recent_notification_id"], mention_api_id);
    assert_eq!(mention_group["page_min_id"], mention_api_id.to_string());
    assert_eq!(mention_group["page_max_id"], mention_api_id.to_string());
    assert_eq!(
        mention_group["latest_page_notification_at"],
        "2026-04-19T00:00:00Z"
    );
}

fn favourite_entry(
    id: &str,
    created_at: &str,
    account_id: &str,
    status_id: &str,
) -> NotificationEntry {
    NotificationEntry {
        id: id.to_owned(),
        created_at: created_at.to_owned(),
        value: serde_json::json!({
            "id": id,
            "type": "favourite",
            "account": {"id": account_id},
            "status": {"id": status_id}
        }),
    }
}

#[test]
fn build_notifications_v2_document_groups_favourites_of_one_status() {
    let entries = vec![
        favourite_entry("fav-3", "2026-04-19T10:00:00Z", "carol", "status-1"),
        favourite_entry("fav-2", "2026-04-19T09:00:00Z", "bob", "status-1"),
        favourite_entry("fav-other", "2026-04-19T08:30:00Z", "bob", "status-2"),
        favourite_entry("fav-1", "2026-04-19T08:00:00Z", "alice", "status-1"),
        // Same status, but outside the 12-hour bucket.
        favourite_entry("fav-old", "2026-04-18T08:00:00Z", "dave", "status-1"),
    ];

    let document = build_notifications_v2_document(&entries, &default_grouped_notification_types());
    let groups = document["notification_groups"].as_array().unwrap();
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0]["notifications_count"], 3);
    assert_eq!(
        groups[0]["sample_account_ids"],
        serde_json::json!(["carol", "bob", "alice"])
    );
    assert_eq!(
        groups[0]["page_max_id"],
        notification_api_numeric_id(&entries[0]).to_string()
    );
    assert_eq!(
        groups[0]["page_min_id"],
        notification_api_numeric_id(&entries[3]).to_string()
    );
    assert_eq!(groups[1]["status_id"], "status-2");
    assert_eq!(groups[2]["notifications_count"], 1);
}

#[test]
fn build_notifications_v2_document_leaves_types_outside_grouped_types_ungrouped() {
    let entries = vec![
        favourite_entry("fav-2", "2026-04-19T09:00:00Z", "bob", "status-1"),
        favourite_entry("fav-1", "2026-04-19T08:00:00Z", "alice", "status-1"),
    ];

    let document = build_notifications_v2_document(&entries, &["follow".to_owned()]);
    let groups = document["notification_groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    assert!(
        groups[0]["group_key"]
            .as_str()
            .unwrap()
            .starts_with("ungrouped-")
    );
}

#[test]
fn build_notifications_v2_document_uses_numeric_ids_for_remote_follow_notifications() {
    let entry = NotificationEntry {
        id: "follow-remote-r_aHR0cHM6Ly9ibG9nLmtvc3VpLm1lL3VzZXJzL2tvc3Vp".to_owned(),
        created_at: "2024-05-10 05:16:13".to_owned(),
        value: serde_json::json!({
            "id": "follow-remote-r_aHR0cHM6Ly9ibG9nLmtvc3VpLm1lL3VzZXJzL2tvc3Vp",
            "type": "follow",
            "group_key": "follow-remote-r_aHR0cHM6Ly9ibG9nLmtvc3VpLm1lL3VzZXJzL2tvc3Vp",
            "account": {
                "id": "r_aHR0cHM6Ly9ibG9nLmtvc3VpLm1lL3VzZXJzL2tvc3Vp",
                "acct": "kosui@blog.kosui.me"
            }
        }),
    };

    let document = build_notifications_v2_document(
        std::slice::from_ref(&entry),
        &default_grouped_notification_types(),
    );
    let group = &document["notification_groups"][0];
    assert!(group["most_recent_notification_id"].is_number());
    assert_eq!(
        group["most_recent_notification_id"],
        notification_api_numeric_id(&entry)
    );
    assert!(group["page_min_id"].is_string());
    assert!(group["page_max_id"].is_string());
    assert_eq!(
        group["latest_page_notification_at"],
        "2024-05-10T05:16:13.000Z"
    );
}

#[test]
fn is_admin_account_matches_configured_emails() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.admin_emails = vec!["admin@example.com".to_owned()];
    let mut record = LocalAccountRecord::test_fixture("acct-1", "alice");
    record.access_email = "admin@example.com".to_owned();
    let account = LocalAccount::from_record(record);
    assert!(is_admin_account(&config, &account));

    let mut record = LocalAccountRecord::test_fixture("acct-1", "alice");
    record.access_email = "user@example.com".to_owned();
    let account = LocalAccount::from_record(record);
    assert!(!is_admin_account(&config, &account));
}

#[test]
fn is_admin_authorized_accepts_auth0_roles_or_admin_emails() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.auth0_admin_roles = vec!["admin".to_owned()];
    let mut record = LocalAccountRecord::test_fixture("acct-1", "alice");
    record.access_email = "user@example.com".to_owned();
    let account = LocalAccount::from_record(record);

    assert!(is_admin_authorized(
        &config,
        &account,
        &["admin".to_owned()]
    ));
    assert!(is_admin_authorized(
        &config,
        &account,
        &["ADMIN".to_owned()]
    ));
    assert!(!is_admin_authorized(
        &config,
        &account,
        &["moderator".to_owned()]
    ));

    config.admin_emails = vec!["user@example.com".to_owned()];
    assert!(is_admin_authorized(&config, &account, &[]));
}

#[test]
fn notification_timestamp_sort_token_supports_sqlite_and_iso_shapes() {
    assert!(notification_timestamp_sort_token("2026-04-14 12:34:56").is_some());
    assert!(notification_timestamp_sort_token("2026-04-14T12:34:56.000Z").is_some());
    assert!(notification_timestamp_sort_token("not-a-date").is_none());
}

#[test]
fn notification_api_ids_order_across_timestamp_formats() {
    let newer = favourite_entry("a", "2026-04-14T12:34:56.000Z", "x", "s");
    let older = favourite_entry("b", "2026-04-14 12:33:56", "x", "s");
    assert!(notification_api_numeric_id(&newer) > notification_api_numeric_id(&older));
}

#[test]
fn filter_notification_entries_by_query_applies_max_and_min_cursor() {
    let entries = vec![
        NotificationEntry {
            id: "notif-new".to_owned(),
            created_at: "2026-04-19T12:00:00.000Z".to_owned(),
            value: serde_json::json!({"id": "notif-new"}),
        },
        NotificationEntry {
            id: "notif-mid".to_owned(),
            created_at: "2026-04-19T11:00:00.000Z".to_owned(),
            value: serde_json::json!({"id": "notif-mid"}),
        },
        NotificationEntry {
            id: "notif-old".to_owned(),
            created_at: "2026-04-19T10:00:00.000Z".to_owned(),
            value: serde_json::json!({"id": "notif-old"}),
        },
    ];

    let older_than_mid = filter_notification_entries_by_query(
        entries.clone(),
        &NotificationsQuery {
            max_id: Some("notif-mid".to_owned()),
            ..NotificationsQuery::default()
        },
    );
    assert_eq!(
        older_than_mid
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec!["notif-old".to_owned()]
    );

    let newer_than_mid = filter_notification_entries_by_query(
        entries.clone(),
        &NotificationsQuery {
            min_id: Some("notif-mid".to_owned()),
            ..NotificationsQuery::default()
        },
    );
    assert_eq!(
        newer_than_mid
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec!["notif-new".to_owned()]
    );

    let mid_entry = &entries[1];
    let older_than_mid_numeric = filter_notification_entries_by_query(
        entries.clone(),
        &NotificationsQuery {
            max_id: Some(notification_api_numeric_id(mid_entry).to_string()),
            ..NotificationsQuery::default()
        },
    );
    assert_eq!(
        older_than_mid_numeric
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec!["notif-old".to_owned()]
    );
}

#[test]
fn unread_count_only_counts_notifications_newer_than_the_marker() {
    let entries = vec![
        favourite_entry("n3", "2026-04-19T10:00:00Z", "carol", "status-1"),
        favourite_entry("n2", "2026-04-19T09:00:00Z", "bob", "status-1"),
        favourite_entry("n1", "2026-04-19T08:00:00Z", "alice", "status-1"),
    ];
    assert_eq!(count_unread_notification_entries(&entries, Some("n2")), 1);
    let numeric = notification_api_numeric_id(&entries[0]).to_string();
    assert_eq!(
        count_unread_notification_entries(&entries, Some(&numeric)),
        0
    );
    assert_eq!(count_unread_notification_entries(&entries, None), 3);
    assert_eq!(count_unread_notification_entries(&entries, Some("gone")), 3);
}

#[test]
fn notification_api_ids_are_numeric_time_ordered_and_stable() {
    let older = favourite_entry("favourite-remote-a-1", "2026-04-19T08:00:00Z", "a", "s");
    let newer = favourite_entry("favourite-remote-b-1", "2026-04-19T08:00:01Z", "b", "s");
    let older_id = notification_api_numeric_id(&older);
    assert!(older_id > 0);
    assert!(notification_api_numeric_id(&newer) > older_id);
    assert_eq!(notification_api_numeric_id(&older), older_id);
    // Snowflake layout: creation time in milliseconds above 16 low bits.
    assert_eq!(older_id >> 16, 1_776_585_600_000);
}

#[test]
fn notification_v1_value_exposes_api_id_and_group_key() {
    let entry = favourite_entry(
        "favourite-remote-a-1",
        "2026-04-19T08:00:00Z",
        "a",
        "status-1",
    );
    let value = notification_v1_value(&entry);
    assert_eq!(value["id"], notification_api_numeric_id(&entry).to_string());
    assert!(
        value["group_key"]
            .as_str()
            .unwrap()
            .starts_with("favourite-status-1-")
    );
}

#[test]
fn numeric_cursor_bounds_page_without_finding_its_entry() {
    let entries = vec![
        favourite_entry("n3", "2026-04-19T10:00:00Z", "c", "s"),
        favourite_entry("n2", "2026-04-19T09:00:00Z", "b", "s"),
        favourite_entry("n1", "2026-04-19T08:00:00Z", "a", "s"),
    ];
    // A cursor for a notification no longer in the window, between n2 and n3.
    let gone = favourite_entry("gone", "2026-04-19T09:30:00Z", "x", "s");
    let query = NotificationsQuery {
        max_id: Some(notification_api_numeric_id(&gone).to_string()),
        ..NotificationsQuery::default()
    };
    let page = filter_notification_entries_by_query(entries.clone(), &query);
    assert_eq!(
        page.iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["n2", "n1"]
    );
    let marker = notification_api_numeric_id(&gone).to_string();
    assert_eq!(
        count_unread_notification_entries(&entries, Some(&marker)),
        1
    );
}

#[test]
fn legacy_hash_ids_still_resolve_inside_the_window() {
    let entries = vec![
        favourite_entry("n2", "2026-04-19T09:00:00Z", "b", "s"),
        favourite_entry("n1", "2026-04-19T08:00:00Z", "a", "s"),
    ];
    // The pre-snowflake id of "n2": FNV-1a hash folded into [1e15, 9e15).
    let mut hash = 14695981039346656037_u64;
    for byte in b"n2" {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    let legacy = (1_000_000_000_000_000_u64 + hash % 8_000_000_000_000_000).to_string();
    assert_eq!(
        count_unread_notification_entries(&entries, Some(&legacy)),
        0
    );
    let query = NotificationsQuery {
        max_id: Some(legacy),
        ..NotificationsQuery::default()
    };
    let page = filter_notification_entries_by_query(entries, &query);
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, "n1");
}
