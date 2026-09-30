use crate::responses::{MastodonAccountResponse, MastodonReportResponse};
use crate::test_fixtures::actor_fixture_account_locked_bot;
use cfwdon_core::AppConfig;

#[test]
fn mastodon_report_response_serializes_forwarded_and_nullable_status_ids() {
    let target_account = MastodonAccountResponse {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        acct: "alice".to_owned(),
        uri: "https://social.example/users/alice".to_owned(),
        display_name: "Alice".to_owned(),
        locked: false,
        bot: false,
        group: false,
        discoverable: true,
        indexable: true,
        noindex: None,
        hide_collections: None,
        show_media: Some(true),
        show_media_replies: Some(true),
        show_featured: Some(true),
        last_status_at: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        note: String::new(),
        url: "https://social.example/@alice".to_owned(),
        avatar: String::new(),
        avatar_static: String::new(),
        avatar_description: String::new(),
        header: String::new(),
        header_static: String::new(),
        header_description: String::new(),
        emojis: Vec::new(),
        fields: Vec::new(),
        roles: Some(Vec::new()),
        feature_approval: serde_json::json!({
            "automatic": ["public"],
            "manual": [],
            "current_user": "automatic",
        }),
        followers_count: 0,
        following_count: 0,
        statuses_count: 0,
        source: None,
        role: None,
    };
    let response = MastodonReportResponse {
        id: "report-1".to_owned(),
        action_taken: false,
        action_taken_at: None,
        category: "other".to_owned(),
        comment: "context".to_owned(),
        forwarded: false,
        created_at: "2026-01-02T00:00:00.000Z".to_owned(),
        status_ids: None,
        collection_ids: Some(Vec::new()),
        target_account,
        rule_ids: None,
    };

    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["forwarded"], serde_json::json!(false));
    assert!(value.get("forward").is_none());
    assert_eq!(value["status_ids"], serde_json::Value::Null);
    assert_eq!(value["rule_ids"], serde_json::Value::Null);
}

#[test]
fn mastodon_account_response_reflects_locked_and_bot_flags() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let account = actor_fixture_account_locked_bot();

    let response = MastodonAccountResponse::from_account(&account, &config);
    assert!(response.locked);
    assert!(response.bot);
    assert_eq!(response.discoverable, account.is_discoverable());
}
