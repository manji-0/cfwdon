use super::{
    StatusRow, actor_url, add_seconds_to_iso_string, generate_entity_id, now_iso_string,
    render_status_html, require_status_by_id,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::{LocalAccount, PollDraft, StatusDraft};
use worker::d1::D1Type;
use worker::{D1Database, Result};

pub(crate) async fn insert_status(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    draft: &StatusDraft,
    quote_of_uri: Option<&str>,
) -> Result<StatusRow> {
    let status_id = generate_entity_id(16)?;
    let ap_id = format!(
        "{}/statuses/{}",
        actor_url(config, &account.username),
        status_id
    );
    let content_html = render_status_html(&draft.text);
    let created_at = now_iso_string()?;

    let id = D1Type::Text(status_id.as_str());
    let account_id = D1Type::Text(account.id.as_str());
    let ap_id_binding = D1Type::Text(ap_id.as_str());
    let in_reply_to_id = match draft.in_reply_to_id.as_deref() {
        Some(value) => D1Type::Text(value),
        None => D1Type::Null,
    };
    let content_html_binding = D1Type::Text(content_html.as_str());
    let text_content = D1Type::Text(draft.text.as_str());
    let visibility = D1Type::Text(draft.visibility.as_str());
    let sensitive = D1Type::Integer(if draft.sensitive { 1 } else { 0 });
    let created_at_binding = D1Type::Text(created_at.as_str());
    let spoiler_text = D1Type::Text(draft.spoiler_text.as_str());
    let language = match draft.language.as_deref() {
        Some(value) => D1Type::Text(value),
        None => D1Type::Null,
    };

    let bindings = [
        id,
        account_id,
        ap_id_binding,
        in_reply_to_id,
        D1Type::Null,
        quote_of_uri.map_or(D1Type::Null, D1Type::Text),
        content_html_binding,
        text_content,
        spoiler_text,
        visibility,
        sensitive,
        language,
        created_at_binding,
    ];

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
            ?13
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    if let Some(poll) = draft.poll.as_ref() {
        insert_status_poll(db, &status_id, poll, &created_at).await?;
    }

    require_status_by_id(db, &status_id).await
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
        let bindings = [
            D1Type::Text(existing.id.as_str()),
            D1Type::Text(visibility),
            D1Type::Text(updated_at.as_str()),
        ];
        db.prepare(
            "UPDATE statuses
             SET visibility = ?2,
                 updated_at = ?3
             WHERE id = ?1",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
        return require_status_by_id(db, &existing.id).await;
    }

    let status_id = generate_entity_id(16)?;
    let ap_id = format!(
        "{}/statuses/{}",
        actor_url(config, &account.username),
        status_id
    );
    let created_at = now_iso_string()?;
    let bindings = [
        D1Type::Text(status_id.as_str()),
        D1Type::Text(account.id.as_str()),
        D1Type::Text(ap_id.as_str()),
        D1Type::Null,
        D1Type::Text(target_uri),
        D1Type::Null,
        D1Type::Text(""),
        D1Type::Text(""),
        D1Type::Text(""),
        D1Type::Text(visibility),
        D1Type::Integer(0),
        D1Type::Null,
        D1Type::Text(created_at.as_str()),
    ];
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
            ?13
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    require_status_by_id(db, &status_id).await
}

pub(crate) async fn find_reblog_wrapper_status_by_target_uri(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<Option<StatusRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
    db.prepare(
        "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
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
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
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

pub(crate) async fn insert_status_poll(
    db: &D1Database,
    status_id: &str,
    poll: &PollDraft,
    created_at: &str,
) -> Result<()> {
    let poll_id = generate_entity_id(16)?;
    let expires_at = add_seconds_to_iso_string(created_at, poll.expires_in_seconds)?;
    let bindings = [
        D1Type::Text(poll_id.as_str()),
        D1Type::Text(status_id),
        D1Type::Integer(if poll.multiple { 1 } else { 0 }),
        D1Type::Integer(if poll.hide_totals { 1 } else { 0 }),
        D1Type::Text(expires_at.as_str()),
        D1Type::Text(created_at),
    ];
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
        let bindings = [
            D1Type::Text(option_id.as_str()),
            D1Type::Text(poll_id.as_str()),
            D1Type::Text(option.as_str()),
            D1Type::Integer(position as i32),
        ];
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

pub(crate) async fn delete_status_poll(db: &D1Database, status_id: &str) -> Result<()> {
    let status_binding = D1Type::Text(status_id);
    db.prepare(
        "DELETE FROM status_poll_options
         WHERE poll_id IN (
             SELECT id
             FROM status_polls
             WHERE status_id = ?1
         )",
    )
    .bind_refs(&status_binding)?
    .run()
    .await?;

    let status_binding = D1Type::Text(status_id);
    db.prepare(
        "DELETE FROM status_polls
         WHERE status_id = ?1",
    )
    .bind_refs(&status_binding)?
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

pub(crate) async fn delete_status_by_id(db: &D1Database, status_id: &str) -> Result<()> {
    delete_status_poll(db, status_id).await?;

    let status_id = D1Type::Text(status_id);
    db.prepare(
        "DELETE FROM statuses
         WHERE id = ?1",
    )
    .bind_refs(&status_id)?
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
    let bindings = [
        D1Type::Text(content_html.as_str()),
        D1Type::Text(text),
        D1Type::Text(spoiler_text),
        D1Type::Integer(if sensitive { 1 } else { 0 }),
        match language {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(updated_at),
        D1Type::Text(status.id.as_str()),
    ];
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

    require_status_by_id(db, &status.id).await
}

pub(crate) async fn clear_local_status_quote(
    db: &D1Database,
    status: &StatusRow,
    updated_at: &str,
) -> Result<StatusRow> {
    let bindings = [D1Type::Text(updated_at), D1Type::Text(status.id.as_str())];
    db.prepare(
        "UPDATE statuses
         SET quote_of_uri = NULL,
             updated_at = ?1
         WHERE id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    require_status_by_id(db, &status.id).await
}
