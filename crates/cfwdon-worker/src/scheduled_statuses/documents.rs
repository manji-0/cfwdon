use super::ScheduledStatus;
use crate::{
    AppConfig, D1Database, MastodonMediaAttachmentResponse, Result, StatusDraft,
    find_media_attachment_by_id,
};

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn scheduled_status_document(id: &str) -> serde_json::Value {
    scheduled_status_document_with_params(id, "2099-01-01T00:00:00.000Z", None)
}

pub(crate) fn scheduled_status_document_with_params(
    id: &str,
    scheduled_at: &str,
    draft: Option<&StatusDraft>,
) -> serde_json::Value {
    let poll = draft
        .and_then(|draft| draft.poll().map(serde_json::to_value).transpose().ok())
        .flatten()
        .unwrap_or(serde_json::Value::Null);
    let media_ids = draft
        .map(|draft| draft.media_ids().to_vec())
        .filter(|media_ids| !media_ids.is_empty())
        .map(serde_json::Value::from)
        .unwrap_or(serde_json::Value::Null);
    let language = draft
        .and_then(|draft| draft.language().map(str::to_owned))
        .map(serde_json::Value::from)
        .unwrap_or(serde_json::Value::Null);
    let sensitive = draft
        .map(|draft| serde_json::Value::Bool(draft.sensitive()))
        .unwrap_or(serde_json::Value::Null);
    let visibility = draft
        .map(|draft| serde_json::Value::from(draft.visibility().as_str()))
        .unwrap_or(serde_json::Value::Null);
    let spoiler_text = draft
        .map(|draft| serde_json::Value::from(draft.spoiler_text()))
        .unwrap_or(serde_json::Value::Null);
    let in_reply_to_id = draft
        .and_then(|draft| draft.in_reply_to_id().map(str::to_owned))
        .map(serde_json::Value::from)
        .unwrap_or(serde_json::Value::Null);
    let text = draft
        .map(|draft| draft.text().to_owned())
        .unwrap_or_default();

    serde_json::json!({
        "id": id,
        "scheduled_at": scheduled_at,
        "params": {
            "poll": poll,
            "text": text,
            "language": language,
            "media_ids": media_ids,
            "sensitive": sensitive,
            "visibility": visibility,
            "idempotency": serde_json::Value::Null,
            "scheduled_at": serde_json::Value::Null,
            "spoiler_text": spoiler_text,
            "application_id": 0,
            "in_reply_to_id": in_reply_to_id,
            "with_rate_limit": false,
        },
        "media_attachments": [],
    })
}

pub(in crate::scheduled_statuses) fn build_scheduled_status_document_with_media(
    status: &ScheduledStatus,
    media_attachments: Vec<serde_json::Value>,
) -> serde_json::Value {
    let mut document = scheduled_status_document_with_params(
        &status.id,
        &status.scheduled_at,
        Some(&status.draft),
    );
    if let Some(params) = document
        .get_mut("params")
        .and_then(serde_json::Value::as_object_mut)
    {
        params.insert(
            "idempotency".to_owned(),
            status
                .idempotency_key
                .as_ref()
                .map(|value| serde_json::Value::from(value.clone()))
                .unwrap_or(serde_json::Value::Null),
        );
        params.insert(
            "application_id".to_owned(),
            status
                .application_id
                .map(serde_json::Value::from)
                .unwrap_or(serde_json::json!(0)),
        );
    }
    if let Some(object) = document.as_object_mut() {
        object.insert(
            "media_attachments".to_owned(),
            serde_json::Value::Array(media_attachments),
        );
    }
    document
}

pub(in crate::scheduled_statuses) async fn load_scheduled_media_attachments(
    db: &D1Database,
    config: &AppConfig,
    media_ids: &[String],
) -> Result<Vec<serde_json::Value>> {
    let mut attachments = Vec::new();
    for media_id in media_ids {
        let Some(media) = find_media_attachment_by_id(db, media_id).await? else {
            continue;
        };
        attachments.push(
            serde_json::to_value(MastodonMediaAttachmentResponse::from_row(&media, config))
                .unwrap_or(serde_json::Value::Null),
        );
    }
    Ok(attachments)
}

pub(in crate::scheduled_statuses) async fn build_scheduled_status_document(
    db: &D1Database,
    config: &AppConfig,
    status: &ScheduledStatus,
) -> Result<serde_json::Value> {
    let attachments =
        load_scheduled_media_attachments(db, config, status.draft.media_ids()).await?;
    Ok(build_scheduled_status_document_with_media(
        status,
        attachments,
    ))
}
