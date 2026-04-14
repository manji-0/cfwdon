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
            ?11
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

async fn insert_status_poll(
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

pub(crate) async fn delete_status_by_id(db: &D1Database, status_id: &str) -> Result<()> {
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
