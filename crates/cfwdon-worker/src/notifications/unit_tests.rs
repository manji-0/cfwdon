use crate::notifications::{
    NotificationEntry, NotificationsQuery, build_notifications_v2_document,
    filter_notification_entries_by_query, is_admin_account, is_admin_authorized,
    notification_api_numeric_id, notification_sort_key, notification_timestamp_sort_token,
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

    let document = build_notifications_v2_document(&entries);
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

    let document = build_notifications_v2_document(std::slice::from_ref(&entry));
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
fn notification_sort_key_orders_newer_timestamps_higher() {
    assert!(
        notification_sort_key("2026-04-14T12:34:56.000Z")
            > notification_sort_key("2026-04-14 12:33:56")
    );
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
