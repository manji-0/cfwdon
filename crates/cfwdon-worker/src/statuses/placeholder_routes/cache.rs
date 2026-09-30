use crate::tracked_d1::D1Database;
use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;

#[derive(Debug, Deserialize)]
pub(super) struct TranslationCacheRow {
    source_fingerprint: String,
    translation_json: String,
}

pub(crate) fn translation_cache_source_fingerprint(
    status: &serde_json::Value,
) -> std::result::Result<String, serde_json::Error> {
    let media_attachments = status
        .get("media_attachments")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    serde_json::json!({
                        "id": item.get("id").cloned().unwrap_or_else(|| serde_json::json!("")),
                        "description": item
                            .get("description")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let poll_options = status
        .pointer("/poll/options")
        .and_then(serde_json::Value::as_array)
        .map(|options| {
            options
                .iter()
                .map(|option| {
                    serde_json::json!({
                        "title": option
                            .get("title")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    serde_json::to_string(&serde_json::json!({
        "content": status
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        "spoiler_text": status
            .get("spoiler_text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        "language": status
            .get("language")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("und"),
        "media_attachments": media_attachments,
        "poll_options": poll_options,
    }))
}

pub(super) async fn find_cached_translation_document(
    db: &D1Database,
    status_id: &str,
    target_language: &str,
    provider: &str,
    source_fingerprint: &str,
) -> Result<Option<serde_json::Value>> {
    let bindings = [
        D1Type::Text(status_id),
        D1Type::Text(target_language),
        D1Type::Text(provider),
    ];
    let Some(row) = db
        .prepare(
            "SELECT source_fingerprint, translation_json
             FROM status_translation_cache
             WHERE status_id = ?1
               AND target_language = ?2
               AND provider = ?3",
        )
        .bind_refs(bindings.iter())?
        .first::<TranslationCacheRow>(None)
        .await?
    else {
        return Ok(None);
    };
    if row.source_fingerprint != source_fingerprint {
        return Ok(None);
    }
    serde_json::from_str::<serde_json::Value>(&row.translation_json)
        .map(Some)
        .map_err(|error| {
            worker::Error::RustError(format!("failed to decode cached translation: {error}"))
        })
}

pub(super) async fn store_cached_translation_document(
    db: &D1Database,
    status_id: &str,
    target_language: &str,
    provider: &str,
    source_fingerprint: &str,
    document: &serde_json::Value,
    timestamp: &str,
) -> Result<()> {
    let translation_json = serde_json::to_string(document).map_err(|error| {
        worker::Error::RustError(format!("failed to encode cached translation: {error}"))
    })?;
    let bindings = [
        D1Type::Text(status_id),
        D1Type::Text(target_language),
        D1Type::Text(provider),
        D1Type::Text(source_fingerprint),
        D1Type::Text(translation_json.as_str()),
        D1Type::Text(timestamp),
        D1Type::Text(timestamp),
    ];
    db.prepare(
        "INSERT INTO status_translation_cache (
             status_id, target_language, provider, source_fingerprint,
             translation_json, created_at, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(status_id, target_language, provider) DO UPDATE SET
             source_fingerprint = excluded.source_fingerprint,
             translation_json = excluded.translation_json,
             updated_at = excluded.updated_at",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_cache_source_fingerprint_tracks_translatable_fields() {
        let base = serde_json::json!({
            "content": "<p>Hello</p>",
            "spoiler_text": "cw",
            "language": "en",
            "account": { "display_name": "Alice" },
            "media_attachments": [
                { "id": "media-1", "description": "alt text", "url": "https://media.example/1" }
            ],
            "poll": {
                "id": "poll-1",
                "options": [
                    { "title": "One", "votes_count": 1 }
                ]
            }
        });
        let same_translatable_fields = serde_json::json!({
            "content": "<p>Hello</p>",
            "spoiler_text": "cw",
            "language": "en",
            "account": { "display_name": "Changed" },
            "media_attachments": [
                { "id": "media-1", "description": "alt text", "url": "https://media.example/changed" }
            ],
            "poll": {
                "id": "poll-1",
                "options": [
                    { "title": "One", "votes_count": 99 }
                ]
            }
        });
        let edited_content = serde_json::json!({
            "content": "<p>Hello edited</p>",
            "spoiler_text": "cw",
            "language": "en",
            "media_attachments": [
                { "id": "media-1", "description": "alt text" }
            ],
            "poll": {
                "options": [
                    { "title": "One" }
                ]
            }
        });

        assert_eq!(
            translation_cache_source_fingerprint(&base).unwrap(),
            translation_cache_source_fingerprint(&same_translatable_fields).unwrap()
        );
        assert_ne!(
            translation_cache_source_fingerprint(&base).unwrap(),
            translation_cache_source_fingerprint(&edited_content).unwrap()
        );
    }
}
