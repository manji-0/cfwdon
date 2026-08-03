use super::{
    StatusRecord, StatusRow, actor_url, add_seconds_to_iso_string, build_status_card_value,
    enqueue_addressed_create_activity, enqueue_addressed_delete_activity,
    enqueue_direct_create_activity, enqueue_direct_delete_activity,
    find_local_status_by_object_uri, generate_entity_id, now_iso_string,
    outbox_create_insert_statement, outbox_delete_insert_statement, render_status_html,
    replace_local_status_hashtags, replace_local_status_mentions, require_status_by_id,
    status_from_record,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::{
    LocalAccount, LocalReblogPersistenceFacts, LocalStatus, LocalStatusPersistenceFacts, PollDraft,
    QuoteTargetResolution, StatusDraft, StoredLocalReblogIntent, StoredLocalStatusIntent,
    Visibility,
};
use worker::Result;
use worker::d1::D1Type;

use crate::D1Database;
async fn quote_target_resolution(
    db: &D1Database,
    config: &AppConfig,
    quote_of_uri: Option<&str>,
) -> Result<QuoteTargetResolution> {
    let Some(quote_of_uri) = quote_of_uri else {
        return Ok(QuoteTargetResolution::none());
    };
    let target_exists_locally = find_local_status_by_object_uri(db, config, quote_of_uri)
        .await?
        .is_some();
    Ok(QuoteTargetResolution::with_target(target_exists_locally))
}

pub(crate) async fn insert_status(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    draft: &StatusDraft,
    application_id: Option<i64>,
    quote_of_uri: Option<&str>,
    defer_outbox: bool,
    in_reply_to_account_id: Option<String>,
) -> Result<StatusRow> {
    let quote_resolution = quote_target_resolution(db, config, quote_of_uri).await?;
    let publish_intent = draft
        .clone()
        .into_publish_intent(account, quote_resolution)
        .state;
    let status_id = generate_entity_id(16)?;
    let created_at = now_iso_string()?;
    let card_json =
        build_status_card_value(draft.text()).and_then(|v| serde_json::to_string(&v).ok());
    let stored = publish_intent
        .into_stored_intent(LocalStatusPersistenceFacts {
            status_id: status_id.clone(),
            account_id: account.id().to_owned(),
            ap_id: format!(
                "{}/statuses/{}",
                actor_url(config, account.username()),
                status_id
            ),
            quote_of_uri: quote_of_uri.map(str::to_owned),
            content_html: render_status_html(draft.text()),
            application_id,
            in_reply_to_account_id,
            card_json,
            created_at: created_at.clone(),
        })
        .state;

    let preview = LocalStatus::try_from_record(stored.to_record())
        .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let outbox_statement = if defer_outbox {
        None
    } else {
        outbox_create_insert_statement(db, config, account, &preview).await?
    };
    insert_local_status_intent(db, &stored, outbox_statement).await?;

    if !defer_outbox {
        enqueue_direct_create_activity(db, config, account, &preview, None).await?;
        enqueue_addressed_create_activity(db, config, account, &preview, None).await?;
    }

    replace_local_status_hashtags(
        db,
        &stored.status_id,
        &stored.account_id,
        &stored.created_at,
        &stored.text_content,
    )
    .await?;

    replace_local_status_mentions(
        db,
        config,
        &stored.status_id,
        &stored.created_at,
        &stored.text_content,
    )
    .await?;

    if let Some(poll) = draft.poll() {
        insert_status_poll(db, &stored.status_id, poll, &stored.created_at).await?;
    }

    require_status_by_id(db, &stored.status_id).await
}

async fn insert_local_status_intent(
    db: &D1Database,
    intent: &StoredLocalStatusIntent,
    outbox_statement: Option<crate::D1PreparedStatement>,
) -> Result<()> {
    let bindings = local_status_insert_bindings(intent);

    let status_statement = db
        .prepare(
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
            in_reply_to_account_id,
            card_json,
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
            ?17,
            ?18,
            ?18
        )",
        )
        .bind_refs(bindings.iter())?;

    match outbox_statement {
        Some(outbox_statement) => {
            db.batch(vec![status_statement, outbox_statement]).await?;
        }
        None => {
            status_statement.run().await?;
        }
    }
    Ok(())
}

fn local_status_insert_bindings(intent: &StoredLocalStatusIntent) -> [D1Type<'_>; 18] {
    [
        D1Type::Text(intent.status_id.as_str()),
        D1Type::Text(intent.account_id.as_str()),
        D1Type::Text(intent.ap_id.as_str()),
        intent
            .in_reply_to_id
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        D1Type::Null,
        intent
            .quote_of_uri
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(intent.content_html.as_str()),
        D1Type::Text(intent.text_content.as_str()),
        D1Type::Text(intent.spoiler_text.as_str()),
        D1Type::Text(intent.visibility.as_str()),
        D1Type::Integer(i32::from(intent.sensitive)),
        intent
            .language
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(intent.quote_approval_policy.as_str()),
        D1Type::Text(intent.quote_state.as_str()),
        intent.application_id.map_or(D1Type::Null, |value| {
            D1Type::Integer(i32::try_from(value).unwrap_or(i32::MAX))
        }),
        intent
            .in_reply_to_account_id
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        intent
            .card_json
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(intent.created_at.as_str()),
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
        find_reblog_wrapper_status_by_target_uri(db, account.id(), target_uri).await?
    {
        let updated_at = now_iso_string()?;
        update_reblog_wrapper_status_row(db, &existing.id, visibility, &updated_at).await?;
        return require_status_by_id(db, &existing.id).await;
    }

    let status_id = generate_entity_id(16)?;
    let created_at = now_iso_string()?;
    let stored = StoredLocalReblogIntent::new(LocalReblogPersistenceFacts {
        status_id: status_id.clone(),
        account_id: account.id().to_owned(),
        ap_id: format!(
            "{}/statuses/{}",
            actor_url(config, account.username()),
            status_id
        ),
        boost_of_uri: target_uri.to_owned(),
        visibility: Visibility::parse(visibility).unwrap_or(Visibility::Public),
        created_at: created_at.clone(),
    });

    insert_local_reblog_intent(db, &stored).await?;

    require_status_by_id(db, &stored.status_id).await
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

async fn insert_local_reblog_intent(
    db: &D1Database,
    intent: &StoredLocalReblogIntent,
) -> Result<()> {
    let bindings = local_reblog_insert_bindings(intent);

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

fn local_reblog_insert_bindings(intent: &StoredLocalReblogIntent) -> [D1Type<'_>; 16] {
    [
        D1Type::Text(intent.status_id.as_str()),
        D1Type::Text(intent.account_id.as_str()),
        D1Type::Text(intent.ap_id.as_str()),
        D1Type::Null,
        D1Type::Text(intent.boost_of_uri.as_str()),
        D1Type::Null,
        D1Type::Text(""),
        D1Type::Text(""),
        D1Type::Text(""),
        D1Type::Text(intent.visibility.as_str()),
        D1Type::Integer(0),
        D1Type::Null,
        D1Type::Text("public"),
        D1Type::Text("accepted"),
        D1Type::Text(intent.created_at.as_str()),
        D1Type::Text(intent.created_at.as_str()),
    ]
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
    .first::<StatusRecord>(None)
    .await
    .and_then(|row| row.map(status_from_record).transpose())
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
    let expires_at = add_seconds_to_iso_string(created_at, poll.expires_in_seconds())?;
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

    for (position, option) in poll.options().iter().enumerate() {
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
        D1Type::Integer(if poll.multiple() { 1 } else { 0 }),
        D1Type::Integer(if poll.hide_totals() { 1 } else { 0 }),
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

pub(crate) async fn delete_local_status_with_outbox(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<()> {
    enqueue_direct_delete_activity(db, config, account, status).await?;
    enqueue_addressed_delete_activity(db, config, account, status).await?;

    let mut statements = Vec::new();
    if let Some(statement) = outbox_delete_insert_statement(db, config, account, status).await? {
        statements.push(statement);
    }

    let bindings = status_id_delete_bindings(&status.id);
    statements.push(
        db.prepare(
            "DELETE FROM status_poll_options
             WHERE poll_id IN (
                 SELECT id
                 FROM status_polls
                 WHERE status_id = ?1
             )",
        )
        .bind_refs(bindings.iter())?,
    );
    statements.push(
        db.prepare(
            "DELETE FROM status_polls
             WHERE status_id = ?1",
        )
        .bind_refs(bindings.iter())?,
    );
    statements.push(
        db.prepare(
            "DELETE FROM statuses
             WHERE id = ?1",
        )
        .bind_refs(bindings.iter())?,
    );

    db.batch(statements).await?;
    Ok(())
}

pub(crate) async fn update_local_status(
    db: &D1Database,
    config: &AppConfig,
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

    replace_local_status_mentions(db, config, &status.id, &status.created_at, text).await?;

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

fn local_status_quote_state_update_bindings<'a>(
    quote_state: &'a str,
    updated_at: &'a str,
    status_id: &'a str,
) -> [D1Type<'a>; 3] {
    [
        D1Type::Text(quote_state),
        D1Type::Text(updated_at),
        D1Type::Text(status_id),
    ]
}

pub(crate) async fn update_local_status_quote_state(
    db: &D1Database,
    status: &StatusRow,
    quote_state: cfwdon_domain::QuoteState,
    updated_at: &str,
) -> Result<StatusRow> {
    let bindings = local_status_quote_state_update_bindings(
        quote_state.as_str(),
        updated_at,
        status.id.as_str(),
    );
    db.prepare(
        "UPDATE statuses
         SET quote_state = ?1,
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
    update_local_status_quote_state(
        db,
        status,
        cfwdon_domain::QuoteState::quote_state_after_revoke(status.quote_state),
        updated_at,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_domain::Visibility;

    #[test]
    fn local_status_insert_bindings_keep_sql_slot_order_stable() {
        let intent = StoredLocalStatusIntent {
            status_id: "status-1".to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: "https://social.example/users/alice/statuses/status-1".to_owned(),
            in_reply_to_id: Some("reply-1".to_owned()),
            in_reply_to_account_id: None,
            quote_of_uri: Some("https://remote.example/statuses/quote-1".to_owned()),
            content_html: "<p>hello</p>".to_owned(),
            text_content: "hello".to_owned(),
            spoiler_text: "cw".to_owned(),
            visibility: Visibility::Unlisted,
            sensitive: true,
            language: Some("ja".to_owned()),
            quote_approval_policy: cfwdon_domain::QuoteApprovalPolicy::Followers,
            quote_state: cfwdon_domain::QuoteState::Pending,
            application_id: Some(42),
            card_json: None,
            created_at: "2026-01-02T03:04:05.000Z".to_owned(),
        };
        let bindings = local_status_insert_bindings(&intent);

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
        assert!(matches!(bindings[15], D1Type::Null)); // in_reply_to_account_id
        assert!(matches!(bindings[16], D1Type::Null)); // card_json
        assert!(matches!(
            bindings[17],
            D1Type::Text("2026-01-02T03:04:05.000Z")
        ));
    }

    #[test]
    fn local_status_insert_bindings_use_nulls_for_optional_fields() {
        let intent = StoredLocalStatusIntent {
            status_id: "status-1".to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: "https://social.example/users/alice/statuses/status-1".to_owned(),
            in_reply_to_id: None,
            in_reply_to_account_id: None,
            quote_of_uri: None,
            content_html: "<p>hello</p>".to_owned(),
            text_content: "hello".to_owned(),
            spoiler_text: String::new(),
            visibility: Visibility::Public,
            sensitive: false,
            language: None,
            quote_approval_policy: cfwdon_domain::QuoteApprovalPolicy::Public,
            quote_state: cfwdon_domain::QuoteState::Accepted,
            application_id: None,
            card_json: None,
            created_at: "2026-01-02T03:04:05.000Z".to_owned(),
        };
        let bindings = local_status_insert_bindings(&intent);

        assert!(matches!(bindings[3], D1Type::Null));
        assert!(matches!(bindings[5], D1Type::Null));
        assert!(matches!(bindings[10], D1Type::Integer(0)));
        assert!(matches!(bindings[11], D1Type::Null));
        assert!(matches!(bindings[14], D1Type::Null));
        assert!(matches!(bindings[15], D1Type::Null)); // in_reply_to_account_id
        assert!(matches!(bindings[16], D1Type::Null)); // card_json
    }

    #[test]
    fn stored_local_reblog_intent_maps_storage_fields() {
        let intent = StoredLocalReblogIntent::new(LocalReblogPersistenceFacts {
            status_id: "boost-1".to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: "https://social.example/users/alice/statuses/boost-1".to_owned(),
            boost_of_uri: "https://remote.example/statuses/target-1".to_owned(),
            visibility: Visibility::Unlisted,
            created_at: "2026-01-02T03:04:05.000Z".to_owned(),
        });

        assert_eq!(intent.status_id, "boost-1");
        assert_eq!(intent.account_id, "acct-1");
        assert_eq!(
            intent.ap_id,
            "https://social.example/users/alice/statuses/boost-1"
        );
        assert_eq!(
            intent.boost_of_uri,
            "https://remote.example/statuses/target-1"
        );
        assert_eq!(intent.visibility, Visibility::Unlisted);
        assert_eq!(intent.created_at, "2026-01-02T03:04:05.000Z");
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
    fn local_reblog_insert_bindings_keep_sql_slot_order_stable() {
        let intent = StoredLocalReblogIntent::new(LocalReblogPersistenceFacts {
            status_id: "boost-1".to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: "https://social.example/users/alice/statuses/boost-1".to_owned(),
            boost_of_uri: "https://remote.example/statuses/target-1".to_owned(),
            visibility: Visibility::Unlisted,
            created_at: "2026-01-02T03:04:05.000Z".to_owned(),
        });
        let bindings = local_reblog_insert_bindings(&intent);

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
        let poll = PollDraft::try_new(
            vec!["first".to_owned(), "second".to_owned()],
            300,
            true,
            false,
        )
        .expect("poll draft");
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
    fn local_status_quote_state_update_bindings_keep_sql_slot_order_stable() {
        let bindings = local_status_quote_state_update_bindings(
            "accepted",
            "2026-01-02T03:04:05.000Z",
            "status-1",
        );

        assert!(matches!(bindings[0], D1Type::Text("accepted")));
        assert!(matches!(
            bindings[1],
            D1Type::Text("2026-01-02T03:04:05.000Z")
        ));
        assert!(matches!(bindings[2], D1Type::Text("status-1")));
    }
}
