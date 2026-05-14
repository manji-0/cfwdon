use super::{
    StatusRow, actor_url, add_seconds_to_iso_string, generate_entity_id,
    initial_local_quote_approval_policy, initial_local_quote_state, now_iso_string,
    render_status_html, replace_local_status_hashtags, require_status_by_id,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::{LocalAccount, PollDraft, StatusDraft};
use worker::d1::D1Type;
use worker::{D1Database, Result};

#[derive(Debug, PartialEq, Eq)]
struct LocalStatusInsertDraft {
    status_id: String,
    account_id: String,
    ap_id: String,
    in_reply_to_id: Option<String>,
    quote_of_uri: Option<String>,
    content_html: String,
    text_content: String,
    spoiler_text: String,
    visibility: String,
    sensitive: bool,
    language: Option<String>,
    quote_approval_policy: String,
    quote_state: String,
    application_id: Option<i64>,
    created_at: String,
}

impl LocalStatusInsertDraft {
    fn from_status_draft(
        config: &AppConfig,
        account: &LocalAccount,
        draft: &StatusDraft,
        application_id: Option<i64>,
        quote_of_uri: Option<&str>,
        status_id: String,
        created_at: String,
        quote_state: String,
    ) -> Self {
        Self {
            ap_id: format!(
                "{}/statuses/{}",
                actor_url(config, &account.username),
                status_id
            ),
            status_id,
            account_id: account.id.clone(),
            in_reply_to_id: draft.in_reply_to_id.clone(),
            quote_of_uri: quote_of_uri.map(str::to_owned),
            content_html: render_status_html(&draft.text),
            text_content: draft.text.clone(),
            spoiler_text: draft.spoiler_text.clone(),
            visibility: draft.visibility.as_str().to_owned(),
            sensitive: draft.sensitive,
            language: draft.language.clone(),
            quote_approval_policy: initial_local_quote_approval_policy(account, draft).to_owned(),
            quote_state,
            application_id,
            created_at,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ReblogWrapperStatusInsertDraft {
    status_id: String,
    account_id: String,
    ap_id: String,
    target_uri: String,
    visibility: String,
    created_at: String,
}

impl ReblogWrapperStatusInsertDraft {
    fn from_parts(
        config: &AppConfig,
        account: &LocalAccount,
        target_uri: &str,
        visibility: &str,
        status_id: String,
        created_at: String,
    ) -> Self {
        Self {
            ap_id: format!(
                "{}/statuses/{}",
                actor_url(config, &account.username),
                status_id
            ),
            status_id,
            account_id: account.id.clone(),
            target_uri: target_uri.to_owned(),
            visibility: visibility.to_owned(),
            created_at,
        }
    }
}

pub(crate) async fn insert_status(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    draft: &StatusDraft,
    application_id: Option<i64>,
    quote_of_uri: Option<&str>,
) -> Result<StatusRow> {
    let status_id = generate_entity_id(16)?;
    let created_at = now_iso_string()?;
    let quote_state = initial_local_quote_state(db, config, quote_of_uri).await?;
    let insert_draft = LocalStatusInsertDraft::from_status_draft(
        config,
        account,
        draft,
        application_id,
        quote_of_uri,
        status_id,
        created_at,
        quote_state.to_owned(),
    );

    insert_local_status_row(db, &insert_draft).await?;

    replace_local_status_hashtags(
        db,
        &insert_draft.status_id,
        &insert_draft.account_id,
        &insert_draft.created_at,
        &insert_draft.text_content,
    )
    .await?;

    if let Some(poll) = draft.poll.as_ref() {
        insert_status_poll(db, &insert_draft.status_id, poll, &insert_draft.created_at).await?;
    }

    require_status_by_id(db, &insert_draft.status_id).await
}

async fn insert_local_status_row(db: &D1Database, draft: &LocalStatusInsertDraft) -> Result<()> {
    let bindings = local_status_insert_bindings(draft);

    db.prepare(
        "INSERT INTO statuses (
            id,
            account_id,
            ap_id,
            in_reply_to_id,
            boost_of_uri,
            quote_of_uri,
            content_html,
            text_content,
            spoiler_text,
            visibility,
            sensitive,
            language,
            quote_approval_policy,
            quote_state,
            application_id,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            ?8,
            ?9,
            ?10,
            ?11,
            ?12,
            ?13,
            ?14,
            ?15,
            ?16,
            ?17
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

fn local_status_insert_bindings(draft: &LocalStatusInsertDraft) -> [D1Type<'_>; 17] {
    [
        D1Type::Text(draft.status_id.as_str()),
        D1Type::Text(draft.account_id.as_str()),
        D1Type::Text(draft.ap_id.as_str()),
        draft
            .in_reply_to_id
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        D1Type::Null,
        draft
            .quote_of_uri
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(draft.content_html.as_str()),
        D1Type::Text(draft.text_content.as_str()),
        D1Type::Text(draft.spoiler_text.as_str()),
        D1Type::Text(draft.visibility.as_str()),
        D1Type::Integer(if draft.sensitive { 1 } else { 0 }),
        draft.language.as_deref().map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(draft.quote_approval_policy.as_str()),
        D1Type::Text(draft.quote_state.as_str()),
        draft.application_id.map_or(D1Type::Null, |value| {
            D1Type::Integer(i32::try_from(value).unwrap_or(i32::MAX))
        }),
        D1Type::Text(draft.created_at.as_str()),
        D1Type::Text(draft.created_at.as_str()),
    ]
}

pub(crate) async fn upsert_reblog_wrapper_status(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    target_uri: &str,
    visibility: &str,
) -> Result<StatusRow> {
    if let Some(existing) =
        find_reblog_wrapper_status_by_target_uri(db, &account.id, target_uri).await?
    {
        let updated_at = now_iso_string()?;
        update_reblog_wrapper_status_row(db, &existing.id, visibility, &updated_at).await?;
        return require_status_by_id(db, &existing.id).await;
    }

    let status_id = generate_entity_id(16)?;
    let created_at = now_iso_string()?;
    let insert_draft = ReblogWrapperStatusInsertDraft::from_parts(
        config, account, target_uri, visibility, status_id, created_at,
    );

    insert_reblog_wrapper_status_row(db, &insert_draft).await?;

    require_status_by_id(db, &insert_draft.status_id).await
}

async fn update_reblog_wrapper_status_row(
    db: &D1Database,
    status_id: &str,
    visibility: &str,
    updated_at: &str,
) -> Result<()> {
    let bindings = reblog_wrapper_status_update_bindings(status_id, visibility, updated_at);
    db.prepare(
        "UPDATE statuses
         SET visibility = ?2,
             updated_at = ?3
         WHERE id = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn insert_reblog_wrapper_status_row(
    db: &D1Database,
    draft: &ReblogWrapperStatusInsertDraft,
) -> Result<()> {
    let bindings = reblog_wrapper_status_insert_bindings(draft);
    db.prepare(
        "INSERT INTO statuses (
            id,
            account_id,
            ap_id,
            in_reply_to_id,
            boost_of_uri,
            quote_of_uri,
            content_html,
            text_content,
            spoiler_text,
            visibility,
            sensitive,
            language,
            quote_approval_policy,
            quote_state,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            ?8,
            ?9,
            ?10,
            ?11,
            ?12,
            ?13,
            ?14,
            ?15,
            ?16
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn find_reblog_wrapper_status_by_target_uri(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<Option<StatusRow>> {
    let bindings = reblog_wrapper_status_target_bindings(account_id, target_uri);
    db.prepare(
        "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, created_at
         FROM statuses
         WHERE account_id = ?1
           AND boost_of_uri = ?2
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<StatusRow>(None)
    .await
}

pub(crate) async fn delete_reblog_wrapper_status_by_target_uri(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<()> {
    let bindings = reblog_wrapper_status_target_bindings(account_id, target_uri);
    db.prepare(
        "DELETE FROM statuses
         WHERE account_id = ?1
           AND boost_of_uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

fn reblog_wrapper_status_update_bindings<'a>(
    status_id: &'a str,
    visibility: &'a str,
    updated_at: &'a str,
) -> [D1Type<'a>; 3] {
    [
        D1Type::Text(status_id),
        D1Type::Text(visibility),
        D1Type::Text(updated_at),
    ]
}

fn reblog_wrapper_status_insert_bindings<'a>(
    draft: &'a ReblogWrapperStatusInsertDraft,
) -> [D1Type<'a>; 16] {
    [
        D1Type::Text(draft.status_id.as_str()),
        D1Type::Text(draft.account_id.as_str()),
        D1Type::Text(draft.ap_id.as_str()),
        D1Type::Null,
        D1Type::Text(draft.target_uri.as_str()),
        D1Type::Null,
        D1Type::Text(""),
        D1Type::Text(""),
        D1Type::Text(""),
        D1Type::Text(draft.visibility.as_str()),
        D1Type::Integer(0),
        D1Type::Null,
        D1Type::Text("public"),
        D1Type::Text("accepted"),
        D1Type::Text(draft.created_at.as_str()),
        D1Type::Text(draft.created_at.as_str()),
    ]
}

fn reblog_wrapper_status_target_bindings<'a>(
    account_id: &'a str,
    target_uri: &'a str,
) -> [D1Type<'a>; 2] {
    [D1Type::Text(account_id), D1Type::Text(target_uri)]
}

pub(crate) async fn insert_status_poll(
    db: &D1Database,
    status_id: &str,
    poll: &PollDraft,
    created_at: &str,
) -> Result<()> {
    let poll_id = generate_entity_id(16)?;
    let expires_at = add_seconds_to_iso_string(created_at, poll.expires_in_seconds)?;
    let bindings = status_poll_insert_bindings(&poll_id, status_id, poll, &expires_at, created_at);
    db.prepare(
        "INSERT INTO status_polls (
            id,
            status_id,
            multiple,
            hide_totals,
            expires_at,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?6
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    for (position, option) in poll.options.iter().enumerate() {
        let option_id = generate_entity_id(16)?;
        let bindings = status_poll_option_insert_bindings(&option_id, &poll_id, option, position);
        db.prepare(
            "INSERT INTO status_poll_options (
                id,
                poll_id,
                title,
                position,
                votes_count,
                created_at
            ) VALUES (
                ?1,
                ?2,
                ?3,
                ?4,
                0,
                CURRENT_TIMESTAMP
            )",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}

fn status_poll_insert_bindings<'a>(
    poll_id: &'a str,
    status_id: &'a str,
    poll: &'a PollDraft,
    expires_at: &'a str,
    created_at: &'a str,
) -> [D1Type<'a>; 6] {
    [
        D1Type::Text(poll_id),
        D1Type::Text(status_id),
        D1Type::Integer(if poll.multiple { 1 } else { 0 }),
        D1Type::Integer(if poll.hide_totals { 1 } else { 0 }),
        D1Type::Text(expires_at),
        D1Type::Text(created_at),
    ]
}

fn status_poll_option_insert_bindings<'a>(
    option_id: &'a str,
    poll_id: &'a str,
    option: &'a str,
    position: usize,
) -> [D1Type<'a>; 4] {
    [
        D1Type::Text(option_id),
        D1Type::Text(poll_id),
        D1Type::Text(option),
        D1Type::Integer(position as i32),
    ]
}

pub(crate) async fn delete_status_poll(db: &D1Database, status_id: &str) -> Result<()> {
    let bindings = status_id_delete_bindings(status_id);
    db.prepare(
        "DELETE FROM status_poll_options
         WHERE poll_id IN (
             SELECT id
             FROM status_polls
             WHERE status_id = ?1
         )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let bindings = status_id_delete_bindings(status_id);
    db.prepare(
        "DELETE FROM status_polls
         WHERE status_id = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn replace_status_poll(
    db: &D1Database,
    status_id: &str,
    poll: &PollDraft,
    updated_at: &str,
) -> Result<()> {
    delete_status_poll(db, status_id).await?;
    insert_status_poll(db, status_id, poll, updated_at).await
}

fn status_id_delete_bindings(status_id: &str) -> [D1Type<'_>; 1] {
    [D1Type::Text(status_id)]
}

fn local_status_update_bindings<'a>(
    content_html: &'a str,
    text: &'a str,
    spoiler_text: &'a str,
    sensitive: bool,
    language: Option<&'a str>,
    updated_at: &'a str,
    status_id: &'a str,
) -> [D1Type<'a>; 7] {
    [
        D1Type::Text(content_html),
        D1Type::Text(text),
        D1Type::Text(spoiler_text),
        D1Type::Integer(if sensitive { 1 } else { 0 }),
        language.map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(updated_at),
        D1Type::Text(status_id),
    ]
}

fn local_status_quote_policy_update_bindings<'a>(
    quote_approval_policy: &'a str,
    updated_at: &'a str,
    status_id: &'a str,
) -> [D1Type<'a>; 3] {
    [
        D1Type::Text(quote_approval_policy),
        D1Type::Text(updated_at),
        D1Type::Text(status_id),
    ]
}

fn local_status_quote_clear_bindings<'a>(
    updated_at: &'a str,
    status_id: &'a str,
) -> [D1Type<'a>; 2] {
    [D1Type::Text(updated_at), D1Type::Text(status_id)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_domain::Visibility;

    fn test_account() -> LocalAccount {
        LocalAccount {
            id: "acct-1".to_owned(),
            username: "alice".to_owned(),
            access_email: "alice@example.com".to_owned(),
            display_name: "Alice".to_owned(),
            bio_html: String::new(),
            bio_text: String::new(),
            fields: Vec::new(),
            locked: false,
            bot: false,
            discoverable: false,
            default_post_visibility: "public".to_owned(),
            default_quote_policy: "followers".to_owned(),
            default_sensitive: false,
            default_language: Some("en".to_owned()),
            avatar_object_key: None,
            avatar_content_type: None,
            header_object_key: None,
            header_content_type: None,
            private_key_jwk: "{}".to_owned(),
            public_key_pem: "pem".to_owned(),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        }
    }

    #[test]
    fn local_status_insert_draft_maps_status_fields() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test");
        let account = test_account();
        let status_draft = StatusDraft {
            text: "hello".to_owned(),
            visibility: Visibility::Unlisted,
            spoiler_text: "cw".to_owned(),
            sensitive: true,
            language: Some("ja".to_owned()),
            quote_approval_policy: None,
            in_reply_to_id: Some("reply-1".to_owned()),
            media_ids: Vec::new(),
            poll: None,
        };

        let insert_draft = LocalStatusInsertDraft::from_status_draft(
            &config,
            &account,
            &status_draft,
            Some(42),
            Some("https://remote.example/statuses/quote-1"),
            "status-1".to_owned(),
            "2026-01-02T03:04:05.000Z".to_owned(),
            "pending".to_owned(),
        );

        assert_eq!(insert_draft.status_id, "status-1");
        assert_eq!(insert_draft.account_id, "acct-1");
        assert_eq!(
            insert_draft.ap_id,
            "https://social.example/users/alice/statuses/status-1"
        );
        assert_eq!(insert_draft.in_reply_to_id.as_deref(), Some("reply-1"));
        assert_eq!(
            insert_draft.quote_of_uri.as_deref(),
            Some("https://remote.example/statuses/quote-1")
        );
        assert_eq!(insert_draft.content_html, "<p>hello</p>");
        assert_eq!(insert_draft.text_content, "hello");
        assert_eq!(insert_draft.spoiler_text, "cw");
        assert_eq!(insert_draft.visibility, "unlisted");
        assert!(insert_draft.sensitive);
        assert_eq!(insert_draft.language.as_deref(), Some("ja"));
        assert_eq!(insert_draft.quote_approval_policy, "followers");
        assert_eq!(insert_draft.quote_state, "pending");
        assert_eq!(insert_draft.application_id, Some(42));
        assert_eq!(insert_draft.created_at, "2026-01-02T03:04:05.000Z");
    }

    #[test]
    fn local_status_insert_bindings_keep_sql_slot_order_stable() {
        let insert_draft = LocalStatusInsertDraft {
            status_id: "status-1".to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: "https://social.example/users/alice/statuses/status-1".to_owned(),
            in_reply_to_id: Some("reply-1".to_owned()),
            quote_of_uri: Some("https://remote.example/statuses/quote-1".to_owned()),
            content_html: "<p>hello</p>".to_owned(),
            text_content: "hello".to_owned(),
            spoiler_text: "cw".to_owned(),
            visibility: "unlisted".to_owned(),
            sensitive: true,
            language: Some("ja".to_owned()),
            quote_approval_policy: "followers".to_owned(),
            quote_state: "pending".to_owned(),
            application_id: Some(42),
            created_at: "2026-01-02T03:04:05.000Z".to_owned(),
        };
        let bindings = local_status_insert_bindings(&insert_draft);

        assert!(matches!(bindings[0], D1Type::Text("status-1")));
        assert!(matches!(bindings[1], D1Type::Text("acct-1")));
        assert!(matches!(
            bindings[2],
            D1Type::Text("https://social.example/users/alice/statuses/status-1")
        ));
        assert!(matches!(bindings[3], D1Type::Text("reply-1")));
        assert!(matches!(bindings[4], D1Type::Null));
        assert!(matches!(
            bindings[5],
            D1Type::Text("https://remote.example/statuses/quote-1")
        ));
        assert!(matches!(bindings[6], D1Type::Text("<p>hello</p>")));
        assert!(matches!(bindings[7], D1Type::Text("hello")));
        assert!(matches!(bindings[8], D1Type::Text("cw")));
        assert!(matches!(bindings[9], D1Type::Text("unlisted")));
        assert!(matches!(bindings[10], D1Type::Integer(1)));
        assert!(matches!(bindings[11], D1Type::Text("ja")));
        assert!(matches!(bindings[12], D1Type::Text("followers")));
        assert!(matches!(bindings[13], D1Type::Text("pending")));
        assert!(matches!(bindings[14], D1Type::Integer(42)));
        assert!(matches!(
            bindings[15],
            D1Type::Text("2026-01-02T03:04:05.000Z")
        ));
        assert!(matches!(
            bindings[16],
            D1Type::Text("2026-01-02T03:04:05.000Z")
        ));
    }

    #[test]
    fn local_status_insert_bindings_use_nulls_for_optional_fields() {
        let insert_draft = LocalStatusInsertDraft {
            status_id: "status-1".to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: "https://social.example/users/alice/statuses/status-1".to_owned(),
            in_reply_to_id: None,
            quote_of_uri: None,
            content_html: "<p>hello</p>".to_owned(),
            text_content: "hello".to_owned(),
            spoiler_text: String::new(),
            visibility: "public".to_owned(),
            sensitive: false,
            language: None,
            quote_approval_policy: "public".to_owned(),
            quote_state: "accepted".to_owned(),
            application_id: None,
            created_at: "2026-01-02T03:04:05.000Z".to_owned(),
        };
        let bindings = local_status_insert_bindings(&insert_draft);

        assert!(matches!(bindings[3], D1Type::Null));
        assert!(matches!(bindings[5], D1Type::Null));
        assert!(matches!(bindings[10], D1Type::Integer(0)));
        assert!(matches!(bindings[11], D1Type::Null));
        assert!(matches!(bindings[14], D1Type::Null));
    }

    #[test]
    fn reblog_wrapper_status_insert_draft_maps_storage_fields() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test");
        let account = test_account();

        let insert_draft = ReblogWrapperStatusInsertDraft::from_parts(
            &config,
            &account,
            "https://remote.example/statuses/target-1",
            "unlisted",
            "boost-1".to_owned(),
            "2026-01-02T03:04:05.000Z".to_owned(),
        );

        assert_eq!(insert_draft.status_id, "boost-1");
        assert_eq!(insert_draft.account_id, "acct-1");
        assert_eq!(
            insert_draft.ap_id,
            "https://social.example/users/alice/statuses/boost-1"
        );
        assert_eq!(
            insert_draft.target_uri,
            "https://remote.example/statuses/target-1"
        );
        assert_eq!(insert_draft.visibility, "unlisted");
        assert_eq!(insert_draft.created_at, "2026-01-02T03:04:05.000Z");
    }

    #[test]
    fn reblog_wrapper_status_update_bindings_keep_sql_slot_order_stable() {
        let bindings = reblog_wrapper_status_update_bindings(
            "status-1",
            "unlisted",
            "2026-01-02T03:04:05.000Z",
        );

        assert!(matches!(bindings[0], D1Type::Text("status-1")));
        assert!(matches!(bindings[1], D1Type::Text("unlisted")));
        assert!(matches!(
            bindings[2],
            D1Type::Text("2026-01-02T03:04:05.000Z")
        ));
    }

    #[test]
    fn reblog_wrapper_status_insert_bindings_keep_sql_slot_order_stable() {
        let insert_draft = ReblogWrapperStatusInsertDraft {
            status_id: "boost-1".to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: "https://social.example/users/alice/statuses/boost-1".to_owned(),
            target_uri: "https://remote.example/statuses/target-1".to_owned(),
            visibility: "unlisted".to_owned(),
            created_at: "2026-01-02T03:04:05.000Z".to_owned(),
        };
        let bindings = reblog_wrapper_status_insert_bindings(&insert_draft);

        assert!(matches!(bindings[0], D1Type::Text("boost-1")));
        assert!(matches!(bindings[1], D1Type::Text("acct-1")));
        assert!(matches!(
            bindings[2],
            D1Type::Text("https://social.example/users/alice/statuses/boost-1")
        ));
        assert!(matches!(bindings[3], D1Type::Null));
        assert!(matches!(
            bindings[4],
            D1Type::Text("https://remote.example/statuses/target-1")
        ));
        assert!(matches!(bindings[5], D1Type::Null));
        assert!(matches!(bindings[6], D1Type::Text("")));
        assert!(matches!(bindings[7], D1Type::Text("")));
        assert!(matches!(bindings[8], D1Type::Text("")));
        assert!(matches!(bindings[9], D1Type::Text("unlisted")));
        assert!(matches!(bindings[10], D1Type::Integer(0)));
        assert!(matches!(bindings[11], D1Type::Null));
        assert!(matches!(bindings[12], D1Type::Text("public")));
        assert!(matches!(bindings[13], D1Type::Text("accepted")));
        assert!(matches!(
            bindings[14],
            D1Type::Text("2026-01-02T03:04:05.000Z")
        ));
        assert!(matches!(
            bindings[15],
            D1Type::Text("2026-01-02T03:04:05.000Z")
        ));
    }

    #[test]
    fn reblog_wrapper_status_target_bindings_keep_sql_slot_order_stable() {
        let bindings =
            reblog_wrapper_status_target_bindings("acct-1", "https://remote.example/statuses/1");

        assert!(matches!(bindings[0], D1Type::Text("acct-1")));
        assert!(matches!(
            bindings[1],
            D1Type::Text("https://remote.example/statuses/1")
        ));
    }

    #[test]
    fn status_poll_insert_bindings_keep_sql_slot_order_stable() {
        let poll = PollDraft {
            options: vec!["first".to_owned(), "second".to_owned()],
            expires_in_seconds: 300,
            multiple: true,
            hide_totals: false,
        };
        let bindings = status_poll_insert_bindings(
            "poll-1",
            "status-1",
            &poll,
            "2026-01-02T03:09:05.000Z",
            "2026-01-02T03:04:05.000Z",
        );

        assert!(matches!(bindings[0], D1Type::Text("poll-1")));
        assert!(matches!(bindings[1], D1Type::Text("status-1")));
        assert!(matches!(bindings[2], D1Type::Integer(1)));
        assert!(matches!(bindings[3], D1Type::Integer(0)));
        assert!(matches!(
            bindings[4],
            D1Type::Text("2026-01-02T03:09:05.000Z")
        ));
        assert!(matches!(
            bindings[5],
            D1Type::Text("2026-01-02T03:04:05.000Z")
        ));
    }

    #[test]
    fn status_poll_option_insert_bindings_keep_sql_slot_order_stable() {
        let bindings = status_poll_option_insert_bindings("option-1", "poll-1", "first", 2);

        assert!(matches!(bindings[0], D1Type::Text("option-1")));
        assert!(matches!(bindings[1], D1Type::Text("poll-1")));
        assert!(matches!(bindings[2], D1Type::Text("first")));
        assert!(matches!(bindings[3], D1Type::Integer(2)));
    }

    #[test]
    fn status_id_delete_bindings_keep_sql_slot_order_stable() {
        let bindings = status_id_delete_bindings("status-1");

        assert!(matches!(bindings[0], D1Type::Text("status-1")));
    }

    #[test]
    fn local_status_update_bindings_keep_sql_slot_order_stable() {
        let bindings = local_status_update_bindings(
            "<p>hello</p>",
            "hello",
            "cw",
            true,
            Some("ja"),
            "2026-01-02T03:04:05.000Z",
            "status-1",
        );

        assert!(matches!(bindings[0], D1Type::Text("<p>hello</p>")));
        assert!(matches!(bindings[1], D1Type::Text("hello")));
        assert!(matches!(bindings[2], D1Type::Text("cw")));
        assert!(matches!(bindings[3], D1Type::Integer(1)));
        assert!(matches!(bindings[4], D1Type::Text("ja")));
        assert!(matches!(
            bindings[5],
            D1Type::Text("2026-01-02T03:04:05.000Z")
        ));
        assert!(matches!(bindings[6], D1Type::Text("status-1")));
    }

    #[test]
    fn local_status_update_bindings_use_null_for_missing_language() {
        let bindings = local_status_update_bindings(
            "<p>hello</p>",
            "hello",
            "",
            false,
            None,
            "2026-01-02T03:04:05.000Z",
            "status-1",
        );

        assert!(matches!(bindings[3], D1Type::Integer(0)));
        assert!(matches!(bindings[4], D1Type::Null));
    }

    #[test]
    fn local_status_quote_policy_update_bindings_keep_sql_slot_order_stable() {
        let bindings = local_status_quote_policy_update_bindings(
            "followers",
            "2026-01-02T03:04:05.000Z",
            "status-1",
        );

        assert!(matches!(bindings[0], D1Type::Text("followers")));
        assert!(matches!(
            bindings[1],
            D1Type::Text("2026-01-02T03:04:05.000Z")
        ));
        assert!(matches!(bindings[2], D1Type::Text("status-1")));
    }

    #[test]
    fn local_status_quote_clear_bindings_keep_sql_slot_order_stable() {
        let bindings = local_status_quote_clear_bindings("2026-01-02T03:04:05.000Z", "status-1");

        assert!(matches!(
            bindings[0],
            D1Type::Text("2026-01-02T03:04:05.000Z")
        ));
        assert!(matches!(bindings[1], D1Type::Text("status-1")));
    }
}

pub(crate) async fn delete_status_by_id(db: &D1Database, status_id: &str) -> Result<()> {
    delete_status_poll(db, status_id).await?;

    let bindings = status_id_delete_bindings(status_id);
    db.prepare(
        "DELETE FROM statuses
         WHERE id = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn update_local_status(
    db: &D1Database,
    status: &StatusRow,
    text: &str,
    spoiler_text: &str,
    sensitive: bool,
    language: Option<&str>,
    updated_at: &str,
) -> Result<StatusRow> {
    let content_html = render_status_html(text);
    let bindings = local_status_update_bindings(
        &content_html,
        text,
        spoiler_text,
        sensitive,
        language,
        updated_at,
        &status.id,
    );
    db.prepare(
        "UPDATE statuses
         SET content_html = ?1,
             text_content = ?2,
             spoiler_text = ?3,
             sensitive = ?4,
             language = ?5,
             updated_at = ?6
         WHERE id = ?7",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    replace_local_status_hashtags(db, &status.id, &status.account_id, &status.created_at, text)
        .await?;

    require_status_by_id(db, &status.id).await
}

pub(crate) async fn update_local_status_quote_approval_policy(
    db: &D1Database,
    status: &StatusRow,
    quote_approval_policy: &str,
    updated_at: &str,
) -> Result<StatusRow> {
    let bindings =
        local_status_quote_policy_update_bindings(quote_approval_policy, updated_at, &status.id);
    db.prepare(
        "UPDATE statuses
         SET quote_approval_policy = ?1,
             updated_at = ?2
         WHERE id = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    require_status_by_id(db, &status.id).await
}

pub(crate) async fn clear_local_status_quote(
    db: &D1Database,
    status: &StatusRow,
    updated_at: &str,
) -> Result<StatusRow> {
    let bindings = local_status_quote_clear_bindings(updated_at, &status.id);
    db.prepare(
        "UPDATE statuses
         SET quote_state = 'revoked',
             updated_at = ?1
         WHERE id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    require_status_by_id(db, &status.id).await
}
