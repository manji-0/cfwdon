use super::bindings::{bool_binding, optional_text_binding};
use cfwdon_domain::{StoredRemoteReblogIntent, StoredRemoteStatusIntent};
use worker::d1::D1Type;

pub(super) fn remote_status_upsert_sql() -> &'static str {
    "INSERT INTO remote_statuses (
        id,
        actor_uri,
        object_uri,
        url,
        in_reply_to_uri,
        boost_of_uri,
        quote_of_uri,
        content_html,
        text_content,
        spoiler_text,
        visibility,
        sensitive,
        language,
        quote_state,
        published_at,
        raw_object_json,
        created_at,
        updated_at,
        edited_at,
        card_json,
        federated_emojis_json,
        in_reply_to_id
    ) VALUES (
        ?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
        CURRENT_TIMESTAMP,
        ?16,
        ?17, ?18, ?19, ?20
    )
    ON CONFLICT(object_uri) DO UPDATE SET
        actor_uri = excluded.actor_uri,
        url = excluded.url,
        in_reply_to_uri = excluded.in_reply_to_uri,
        boost_of_uri = excluded.boost_of_uri,
        quote_of_uri = excluded.quote_of_uri,
        content_html = excluded.content_html,
        text_content = excluded.text_content,
        spoiler_text = excluded.spoiler_text,
        visibility = excluded.visibility,
        sensitive = excluded.sensitive,
        language = excluded.language,
        quote_state = CASE
            WHEN remote_statuses.quote_state IN ('rejected', 'revoked')
                THEN remote_statuses.quote_state
            ELSE excluded.quote_state
        END,
        published_at = excluded.published_at,
        raw_object_json = excluded.raw_object_json,
        updated_at = ?16,
        edited_at = excluded.edited_at,
        card_json = CASE
            WHEN excluded.card_json IS NOT NULL THEN excluded.card_json
            ELSE remote_statuses.card_json
        END,
        federated_emojis_json = excluded.federated_emojis_json,
        in_reply_to_id = COALESCE(excluded.in_reply_to_id, remote_statuses.in_reply_to_id)"
}

pub(super) fn remote_status_upsert_bindings(intent: &StoredRemoteStatusIntent) -> [D1Type<'_>; 20] {
    [
        D1Type::Text(intent.status_id.as_str()),
        D1Type::Text(intent.actor_uri.as_str()),
        D1Type::Text(intent.object_uri.as_str()),
        optional_text_binding(intent.url.as_deref()),
        optional_text_binding(intent.in_reply_to_uri.as_deref()),
        optional_text_binding(intent.quote_of_uri.as_deref()),
        D1Type::Text(intent.content_html.as_str()),
        D1Type::Text(intent.text_content.as_str()),
        D1Type::Text(intent.spoiler_text.as_str()),
        D1Type::Text(intent.visibility.as_str()),
        bool_binding(intent.sensitive),
        optional_text_binding(intent.language.as_deref()),
        D1Type::Text(intent.quote_state.as_str()),
        D1Type::Text(intent.published_at.as_str()),
        D1Type::Text(intent.raw_object_json.as_str()),
        D1Type::Text(intent.revision_at.as_str()),
        optional_text_binding(intent.edited_at.as_deref()),
        optional_text_binding(intent.card_json.as_deref()),
        D1Type::Text(intent.federated_emojis_json.as_str()),
        optional_text_binding(intent.in_reply_to_id.as_deref()),
    ]
}

pub(super) fn remote_reblog_upsert_sql() -> &'static str {
    "INSERT INTO remote_statuses (
        id,
        actor_uri,
        object_uri,
        url,
        in_reply_to_uri,
        boost_of_uri,
        quote_of_uri,
        content_html,
        spoiler_text,
        visibility,
        sensitive,
        language,
        quote_state,
        published_at,
        raw_object_json,
        created_at,
        updated_at
    ) VALUES (
        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
        CURRENT_TIMESTAMP,
        CURRENT_TIMESTAMP
    )
    ON CONFLICT(object_uri) DO UPDATE SET
        actor_uri = excluded.actor_uri,
        url = excluded.url,
        in_reply_to_uri = excluded.in_reply_to_uri,
        boost_of_uri = excluded.boost_of_uri,
        quote_of_uri = excluded.quote_of_uri,
        content_html = excluded.content_html,
        spoiler_text = excluded.spoiler_text,
        visibility = excluded.visibility,
        sensitive = excluded.sensitive,
        language = excluded.language,
        quote_state = CASE
            WHEN remote_statuses.quote_state IN ('rejected', 'revoked')
                THEN remote_statuses.quote_state
            ELSE excluded.quote_state
        END,
        published_at = excluded.published_at,
        raw_object_json = excluded.raw_object_json,
        updated_at = CURRENT_TIMESTAMP"
}

pub(super) fn remote_reblog_upsert_bindings(intent: &StoredRemoteReblogIntent) -> [D1Type<'_>; 15] {
    [
        D1Type::Text(intent.status_id.as_str()),
        D1Type::Text(intent.actor_uri.as_str()),
        D1Type::Text(intent.object_uri.as_str()),
        D1Type::Null,
        D1Type::Null,
        D1Type::Text(intent.boost_of_uri.as_str()),
        optional_text_binding(intent.quote_of_uri.as_deref()),
        D1Type::Text(""),
        D1Type::Text(""),
        D1Type::Text(intent.visibility.as_str()),
        D1Type::Integer(0),
        D1Type::Null,
        D1Type::Text(intent.quote_state.as_str()),
        D1Type::Text(intent.published_at.as_str()),
        D1Type::Text(intent.raw_object_json.as_str()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_status_upsert_sql_writes_merged_quote_state() {
        let sql = remote_status_upsert_sql();

        assert!(sql.contains("ON CONFLICT(object_uri) DO UPDATE SET"));
        assert!(sql.contains("WHEN remote_statuses.quote_state IN ('rejected', 'revoked')"));
        assert!(sql.contains("ELSE excluded.quote_state"));
        assert!(sql.contains("updated_at = ?16"));
        assert!(sql.contains("edited_at = excluded.edited_at"));
        assert!(sql.contains("federated_emojis_json = excluded.federated_emojis_json"));
        assert!(sql.contains("in_reply_to_id = COALESCE(excluded.in_reply_to_id"));
    }

    #[test]
    fn remote_reblog_upsert_sql_writes_merged_quote_state() {
        let sql = remote_reblog_upsert_sql();

        assert!(sql.contains("ON CONFLICT(object_uri) DO UPDATE SET"));
        assert!(sql.contains("WHEN remote_statuses.quote_state IN ('rejected', 'revoked')"));
        assert!(sql.contains("ELSE excluded.quote_state"));
        assert!(sql.contains("updated_at = CURRENT_TIMESTAMP"));
    }

    #[test]
    fn remote_status_upsert_bindings_keep_sql_slot_order_stable() {
        use cfwdon_domain::{QuoteState, StatusId, StoredRemoteStatusIntent, Visibility};

        let intent = StoredRemoteStatusIntent {
            status_id: StatusId::new("remote-status-id").expect("status id"),
            actor_uri: "https://remote.example/users/alice".to_owned(),
            object_uri: "https://remote.example/users/alice/statuses/1".to_owned(),
            url: Some("https://remote.example/@alice/1".to_owned()),
            in_reply_to_uri: Some("https://remote.example/users/bob/statuses/9".to_owned()),
            quote_of_uri: Some("https://local.example/users/alice/statuses/2".to_owned()),
            content_html: "<p>Hello</p>".to_owned(),
            text_content: "Hello".to_owned(),
            spoiler_text: "spoiler".to_owned(),
            visibility: Visibility::Public,
            sensitive: true,
            language: Some("ja".to_owned()),
            quote_state: QuoteState::Accepted,
            published_at: "2026-05-10T01:02:03Z".to_owned(),
            edited_at: Some("2026-05-11T00:00:00Z".to_owned()),
            card_json: Some("{\"url\":\"https://example.test\"}".to_owned()),
            federated_emojis_json: "{\"emoji\":true}".to_owned(),
            in_reply_to_id: Some("reply-id".to_owned()),
            raw_object_json: "{\"type\":\"Note\"}".to_owned(),
            revision_at: "revision-time".to_owned(),
        };
        let bindings = remote_status_upsert_bindings(&intent);

        assert!(matches!(bindings[0], D1Type::Text("remote-status-id")));
        assert!(matches!(
            bindings[1],
            D1Type::Text("https://remote.example/users/alice")
        ));
        assert!(matches!(
            bindings[2],
            D1Type::Text("https://remote.example/users/alice/statuses/1")
        ));
        assert!(matches!(
            bindings[3],
            D1Type::Text("https://remote.example/@alice/1")
        ));
        assert!(matches!(
            bindings[4],
            D1Type::Text("https://remote.example/users/bob/statuses/9")
        ));
        assert!(matches!(
            bindings[5],
            D1Type::Text("https://local.example/users/alice/statuses/2")
        ));
        assert!(matches!(bindings[6], D1Type::Text("<p>Hello</p>")));
        assert!(matches!(bindings[7], D1Type::Text("Hello")));
        assert!(matches!(bindings[8], D1Type::Text("spoiler")));
        assert!(matches!(bindings[9], D1Type::Text("public")));
        assert!(matches!(bindings[10], D1Type::Integer(1)));
        assert!(matches!(bindings[11], D1Type::Text("ja")));
        assert!(matches!(bindings[12], D1Type::Text("accepted")));
        assert!(matches!(bindings[13], D1Type::Text("2026-05-10T01:02:03Z")));
        assert!(matches!(bindings[14], D1Type::Text("{\"type\":\"Note\"}")));
        assert!(matches!(bindings[15], D1Type::Text("revision-time")));
        assert!(matches!(bindings[16], D1Type::Text("2026-05-11T00:00:00Z")));
        assert!(matches!(
            bindings[17],
            D1Type::Text("{\"url\":\"https://example.test\"}")
        ));
        assert!(matches!(bindings[18], D1Type::Text("{\"emoji\":true}")));
        assert!(matches!(bindings[19], D1Type::Text("reply-id")));
    }

    #[test]
    fn remote_reblog_upsert_bindings_keep_sql_slot_order_stable() {
        use cfwdon_domain::{QuoteState, StatusId, StoredRemoteReblogIntent, Visibility};

        let intent = StoredRemoteReblogIntent {
            status_id: StatusId::new("remote-reblog-id").expect("status id"),
            actor_uri: "https://remote.example/users/alice".to_owned(),
            object_uri: "https://remote.example/users/alice/activities/announce/1".to_owned(),
            boost_of_uri: "https://remote.example/users/bob/statuses/9".to_owned(),
            quote_of_uri: Some("https://local.example/users/alice/statuses/2".to_owned()),
            visibility: Visibility::Unlisted,
            quote_state: QuoteState::Accepted,
            published_at: "2026-05-11T01:02:03Z".to_owned(),
            raw_object_json: "{\"type\":\"Announce\"}".to_owned(),
        };
        let bindings = remote_reblog_upsert_bindings(&intent);

        assert!(matches!(bindings[0], D1Type::Text("remote-reblog-id")));
        assert!(matches!(
            bindings[1],
            D1Type::Text("https://remote.example/users/alice")
        ));
        assert!(matches!(
            bindings[2],
            D1Type::Text("https://remote.example/users/alice/activities/announce/1")
        ));
        assert!(matches!(bindings[3], D1Type::Null));
        assert!(matches!(bindings[4], D1Type::Null));
        assert!(matches!(
            bindings[5],
            D1Type::Text("https://remote.example/users/bob/statuses/9")
        ));
        assert!(matches!(
            bindings[6],
            D1Type::Text("https://local.example/users/alice/statuses/2")
        ));
        assert!(matches!(bindings[7], D1Type::Text("")));
        assert!(matches!(bindings[8], D1Type::Text("")));
        assert!(matches!(bindings[9], D1Type::Text("unlisted")));
        assert!(matches!(bindings[10], D1Type::Integer(0)));
        assert!(matches!(bindings[11], D1Type::Null));
        assert!(matches!(bindings[12], D1Type::Text("accepted")));
        assert!(matches!(bindings[13], D1Type::Text("2026-05-11T01:02:03Z")));
        assert!(matches!(
            bindings[14],
            D1Type::Text("{\"type\":\"Announce\"}")
        ));
    }
}
