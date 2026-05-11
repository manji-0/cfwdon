use crate::{
    AppConfig, LocalAccount, StatusRow, actor_url, apply_activitypub_poll_fields,
    classify_media_kind, count_poll_voters, find_media_attachments_by_status_id, find_status_by_id,
    find_status_poll_by_status_id, is_iso_timestamp_in_past, list_status_poll_options,
    media_kind_label, media_object_url, status_has_active_quote,
};
use worker::{D1Database, Result};

pub(crate) fn is_public_activitypub_visibility(visibility: &str) -> bool {
    matches!(visibility, "public" | "unlisted")
}

pub(crate) fn local_status_ap_id(
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> String {
    status.ap_id.clone().unwrap_or_else(|| {
        format!(
            "{}/statuses/{}",
            actor_url(config, &account.username),
            status.id
        )
    })
}

pub(crate) fn activitypub_audiences(
    config: &AppConfig,
    username: &str,
    visibility: &str,
) -> (serde_json::Value, serde_json::Value) {
    let public = serde_json::json!(["https://www.w3.org/ns/activitystreams#Public"]);
    let followers = serde_json::json!([format!("{}/followers", actor_url(config, username))]);

    match visibility {
        "unlisted" => (followers, public),
        _ => (public, followers),
    }
}

fn quote_context_mapping() -> serde_json::Value {
    serde_json::json!({
        "_misskey_quote": {
            "@id": "https://misskey-hub.net/ns#_misskey_quote",
            "@type": "@id"
        }
    })
}

fn note_context(include_quote: bool, include_poll: bool) -> serde_json::Value {
    match (include_quote, include_poll) {
        (false, false) => serde_json::json!("https://www.w3.org/ns/activitystreams"),
        (true, false) => serde_json::json!([
            "https://www.w3.org/ns/activitystreams",
            quote_context_mapping()
        ]),
        (false, true) => serde_json::json!([
            "https://www.w3.org/ns/activitystreams",
            {
                "votersCount": "http://joinmastodon.org/ns#votersCount"
            }
        ]),
        (true, true) => serde_json::json!([
            "https://www.w3.org/ns/activitystreams",
            quote_context_mapping(),
            {
                "votersCount": "http://joinmastodon.org/ns#votersCount"
            }
        ]),
    }
}

pub(crate) async fn build_activitypub_note(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
    include_context: bool,
) -> Result<serde_json::Value> {
    let actor = actor_url(config, &account.username);
    let note_id = local_status_ap_id(config, account, status);
    let audiences = activitypub_audiences(config, &account.username, &status.visibility);
    let (poll, reply_uri, attachments) = futures_util::try_join!(
        find_status_poll_by_status_id(db, &status.id),
        async {
            match status.in_reply_to_id.as_deref() {
                Some(reply_id) => Ok(find_status_by_id(db, reply_id)
                    .await?
                    .and_then(|reply| reply.ap_id)),
                None => Ok(None),
            }
        },
        find_media_attachments_by_status_id(db, &status.id),
    )?;
    let has_quote = status_has_active_quote(status);

    let mut note = serde_json::json!({
        "type": "Note",
        "id": note_id.clone(),
        "url": note_id.clone(),
        "attributedTo": actor,
        "content": status.content_html,
        "published": status.created_at,
        "to": audiences.0,
        "cc": audiences.1,
        "attachment": attachments
            .iter()
            .map(|attachment| {
                serde_json::json!({
                    "type": activitypub_media_attachment_type(&attachment.content_type),
                    "mediaType": attachment.content_type,
                    "url": media_object_url(config, &attachment.object_key),
                    "name": if attachment.description.is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(attachment.description.clone())
                    },
                })
            })
            .collect::<Vec<_>>(),
    });

    if include_context {
        note["@context"] = note_context(has_quote, false);
    }
    if !status.spoiler_text.is_empty() {
        note["summary"] = serde_json::json!(status.spoiler_text.clone());
        note["sensitive"] = serde_json::json!(true);
    } else {
        note["sensitive"] = serde_json::json!(status.sensitive != 0);
    }
    if let Some(language) = &status.language {
        let mut content_map = serde_json::Map::new();
        content_map.insert(
            language.clone(),
            serde_json::Value::String(status.content_html.clone()),
        );
        note["contentMap"] = serde_json::Value::Object(content_map);
    }
    if let Some(reply_uri) = reply_uri {
        note["inReplyTo"] = serde_json::json!(reply_uri);
    }
    if has_quote && let Some(quote_uri) = status.quote_of_uri.as_deref() {
        note["quoteUri"] = serde_json::json!(quote_uri);
        note["quoteUrl"] = serde_json::json!(quote_uri);
        note["_misskey_quote"] = serde_json::json!(quote_uri);
    }
    if let Some(poll) = poll {
        let (options, voters_count) = futures_util::try_join!(
            list_status_poll_options(db, &poll.id),
            count_poll_voters(db, &poll.id),
        )?;
        let expired = is_iso_timestamp_in_past(&poll.expires_at).unwrap_or(false);
        apply_activitypub_poll_fields(&mut note, &poll, &options, voters_count, expired);
        if include_context {
            note["@context"] = note_context(has_quote, true);
        }
    }

    Ok(note)
}

pub(crate) fn activitypub_media_attachment_type(content_type: &str) -> &'static str {
    classify_media_kind(content_type)
        .map(media_kind_label)
        .and_then(|kind| match kind {
            "image" => Some("Image"),
            "video" => Some("Video"),
            "audio" => Some("Audio"),
            _ => None,
        })
        .unwrap_or("Document")
}
