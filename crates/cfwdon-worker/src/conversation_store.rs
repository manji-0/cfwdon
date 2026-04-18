use std::collections::HashSet;

use crate::{
    AppConfig, LocalAccount, StatusDraft, StatusRow, extract_account_handles_from_text,
    find_account_by_username, find_remote_actor_by_username_domain, generate_entity_id,
    now_iso_string,
};
use worker::d1::D1Type;
use worker::{D1Database, Result};

#[derive(Debug, serde::Deserialize)]
pub(crate) struct ConversationRow {
    pub(crate) id: String,
    pub(crate) last_status_id: Option<String>,
    pub(crate) unread: i32,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct ConversationParticipantRow {
    pub(crate) participant_ref: String,
}

pub(crate) async fn ensure_direct_conversation_for_status(
    db: &D1Database,
    config: &AppConfig,
    author: &LocalAccount,
    draft: &StatusDraft,
    status: &StatusRow,
) -> Result<Option<String>> {
    if draft.visibility.as_str() != "direct" {
        return Ok(None);
    }

    let conversation_id = match draft.in_reply_to_id.as_deref() {
        Some(in_reply_to_id) => {
            match find_conversation_id_by_status_id(db, in_reply_to_id).await? {
                Some(conversation_id) => conversation_id,
                None => create_conversation(db, &author.id).await?,
            }
        }
        None => create_conversation(db, &author.id).await?,
    };

    let mut participants = HashSet::new();
    participants.insert(author.id.clone());
    for participant in list_conversation_participants(db, &conversation_id).await? {
        participants.insert(participant);
    }
    for handle in extract_account_handles_from_text(&draft.text, config) {
        if handle.is_local_to(&config.instance_domain) {
            if let Some(account) = find_account_by_username(db, &handle.username).await? {
                participants.insert(account.id);
            }
            continue;
        }

        let participant_ref = match handle.domain.as_deref() {
            Some(domain) => {
                match find_remote_actor_by_username_domain(db, &handle.username, domain).await? {
                    Some(actor) => actor.actor_uri,
                    None => format!("{}@{}", handle.username, domain),
                }
            }
            None => continue,
        };
        participants.insert(participant_ref);
    }

    upsert_conversation_state(db, &conversation_id, &author.id, &status.id, false).await?;
    add_conversation_participants(
        db,
        &conversation_id,
        participants.into_iter().collect::<Vec<_>>().as_slice(),
    )
    .await?;
    attach_status_to_conversation(db, &conversation_id, &status.id).await?;
    Ok(Some(conversation_id))
}

async fn create_conversation(db: &D1Database, owner_account_id: &str) -> Result<String> {
    let conversation_id = generate_entity_id(16)?;
    let bindings = [
        D1Type::Text(conversation_id.as_str()),
        D1Type::Text(owner_account_id),
    ];
    db.prepare(
        "INSERT INTO conversations (id, owner_account_id)
         VALUES (?1, ?2)",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(conversation_id)
}

pub(crate) async fn find_conversation_id_by_status_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<String>> {
    let status_id = D1Type::Text(status_id);
    Ok(db
        .prepare(
            "SELECT conversation_id
             FROM conversation_statuses
             WHERE status_id = ?1
             LIMIT 1",
        )
        .bind_refs(&status_id)?
        .first::<serde_json::Value>(None)
        .await?
        .and_then(|value| {
            value
                .get("conversation_id")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        }))
}

pub(crate) async fn list_conversation_participants(
    db: &D1Database,
    conversation_id: &str,
) -> Result<Vec<String>> {
    let conversation_id = D1Type::Text(conversation_id);
    let result = db
        .prepare(
            "SELECT participant_ref
             FROM conversation_participants
             WHERE conversation_id = ?1
             ORDER BY participant_ref ASC",
        )
        .bind_refs(&conversation_id)?
        .all()
        .await?;
    Ok(result
        .results::<ConversationParticipantRow>()?
        .into_iter()
        .map(|row| row.participant_ref)
        .collect())
}

pub(crate) async fn add_conversation_participants(
    db: &D1Database,
    conversation_id: &str,
    participant_refs: &[String],
) -> Result<()> {
    for participant_ref in participant_refs {
        let trimmed = participant_ref.trim();
        if trimmed.is_empty() {
            continue;
        }
        let bindings = [D1Type::Text(conversation_id), D1Type::Text(trimmed)];
        db.prepare(
            "INSERT INTO conversation_participants (conversation_id, participant_ref)
             VALUES (?1, ?2)
             ON CONFLICT(conversation_id, participant_ref) DO NOTHING",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }
    Ok(())
}

pub(crate) async fn attach_status_to_conversation(
    db: &D1Database,
    conversation_id: &str,
    status_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(conversation_id), D1Type::Text(status_id)];
    db.prepare(
        "INSERT INTO conversation_statuses (conversation_id, status_id)
         VALUES (?1, ?2)
         ON CONFLICT(status_id) DO UPDATE SET
             conversation_id = excluded.conversation_id",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn upsert_conversation_state(
    db: &D1Database,
    conversation_id: &str,
    owner_account_id: &str,
    last_status_id: &str,
    unread: bool,
) -> Result<()> {
    let updated_at = now_iso_string()?;
    let bindings = [
        D1Type::Text(conversation_id),
        D1Type::Text(owner_account_id),
        D1Type::Text(last_status_id),
        D1Type::Integer(if unread { 1 } else { 0 }),
        D1Type::Text(updated_at.as_str()),
    ];
    db.prepare(
        "INSERT INTO conversations (id, owner_account_id, last_status_id, unread, deleted_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5)
         ON CONFLICT(id) DO UPDATE SET
             owner_account_id = excluded.owner_account_id,
             last_status_id = excluded.last_status_id,
             unread = excluded.unread,
             deleted_at = NULL,
             updated_at = excluded.updated_at",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn list_conversations_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    max_id: Option<&str>,
    min_id: Option<&str>,
) -> Result<Vec<ConversationRow>> {
    let result = db
        .prepare(
            "SELECT id, last_status_id, unread
             FROM conversations
             WHERE owner_account_id = ?1
               AND deleted_at IS NULL
               AND (?2 IS NULL OR id < ?2)
               AND (?3 IS NULL OR id > ?3)
             ORDER BY updated_at DESC, id DESC
             LIMIT ?4",
        )
        .bind_refs(&[
            D1Type::Text(account_id),
            max_id.map_or(D1Type::Null, D1Type::Text),
            min_id.map_or(D1Type::Null, D1Type::Text),
            D1Type::Integer(limit as i32),
        ])?
        .all()
        .await?;
    result.results::<ConversationRow>()
}

pub(crate) async fn find_conversation_for_account(
    db: &D1Database,
    account_id: &str,
    conversation_id: &str,
) -> Result<Option<ConversationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(conversation_id)];
    db.prepare(
        "SELECT id, last_status_id, unread
         FROM conversations
         WHERE owner_account_id = ?1
           AND id = ?2
           AND deleted_at IS NULL
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<ConversationRow>(None)
    .await
}

pub(crate) async fn mark_conversation_read(
    db: &D1Database,
    account_id: &str,
    conversation_id: &str,
) -> Result<bool> {
    if find_conversation_for_account(db, account_id, conversation_id)
        .await?
        .is_none()
    {
        return Ok(false);
    }
    let updated_at = now_iso_string()?;
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(conversation_id),
        D1Type::Text(updated_at.as_str()),
    ];
    db.prepare(
        "UPDATE conversations
         SET unread = 0,
             updated_at = ?3
         WHERE owner_account_id = ?1
           AND id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(true)
}

pub(crate) async fn delete_conversation_for_account(
    db: &D1Database,
    account_id: &str,
    conversation_id: &str,
) -> Result<bool> {
    if find_conversation_for_account(db, account_id, conversation_id)
        .await?
        .is_none()
    {
        return Ok(false);
    }
    let deleted_at = now_iso_string()?;
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(conversation_id),
        D1Type::Text(deleted_at.as_str()),
    ];
    db.prepare(
        "UPDATE conversations
         SET deleted_at = ?3,
             updated_at = ?3
         WHERE owner_account_id = ?1
           AND id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(true)
}
