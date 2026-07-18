use super::{
    AppConfig, LocalAccount, MediaAttachmentRow, StatusRow, actor_url,
    apply_activitypub_poll_fields, classify_media_kind, count_poll_voters,
    extract_account_handles_from_text, find_account_by_id, find_account_by_username,
    find_local_status_by_object_uri, find_media_attachments_by_status_id,
    find_remote_actor_by_username_domain, find_status_by_id, find_status_poll_by_status_id,
    is_iso_timestamp_in_past, list_status_poll_options, media_attachment_url, media_kind_label,
    quote_authorization_uri, status_has_active_quote,
};
use cfwdon_domain::{QuoteState, Visibility};
use std::collections::HashSet;
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
            actor_url(config, account.username()),
            status.id
        )
    })
}

pub(crate) fn activitypub_audiences_for_visibility(
    config: &AppConfig,
    username: &str,
    visibility: Visibility,
) -> (serde_json::Value, serde_json::Value) {
    let public = serde_json::json!(["https://www.w3.org/ns/activitystreams#Public"]);
    let followers = serde_json::json!([format!("{}/followers", actor_url(config, username))]);

    match visibility {
        Visibility::Public => (public, followers),
        Visibility::Unlisted => (followers, public),
        Visibility::FollowersOnly => (followers, serde_json::json!([])),
        Visibility::Direct => (serde_json::json!([]), serde_json::json!([])),
    }
}

pub(crate) async fn activitypub_audiences_for_status(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<(serde_json::Value, serde_json::Value)> {
    if status.visibility == Visibility::Direct {
        return direct_activitypub_audiences(db, config, account, status).await;
    }

    Ok(activitypub_audiences_for_visibility(
        config,
        account.username(),
        status.visibility,
    ))
}

async fn direct_activitypub_audiences(
    db: &D1Database,
    config: &AppConfig,
    author: &LocalAccount,
    status: &StatusRow,
) -> Result<(serde_json::Value, serde_json::Value)> {
    let mut recipients = HashSet::new();

    for handle in extract_account_handles_from_text(&status.text, config) {
        if handle.is_local_to(&config.instance_domain) {
            if let Some(account) = find_account_by_username(db, &handle.username).await?
                && account.id() != author.id()
            {
                recipients.insert(actor_url(config, account.username()));
            }
            continue;
        }

        if let Some(domain) = handle.domain.as_deref()
            && let Some(actor) =
                find_remote_actor_by_username_domain(db, &handle.username, domain).await?
        {
            recipients.insert(actor.actor_uri);
        }
    }

    if let Some(reply_id) = status.in_reply_to_id.as_deref()
        && let Some(reply) = find_status_by_id(db, reply_id).await?
        && let Some(reply_account) = find_account_by_id(db, &reply.account_id).await?
        && reply_account.id() != author.id()
    {
        recipients.insert(actor_url(config, reply_account.username()));
    }

    let to = serde_json::Value::Array(
        recipients
            .into_iter()
            .map(serde_json::Value::String)
            .collect(),
    );
    Ok((to, serde_json::json!([])))
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
    attachment_override: Option<&[MediaAttachmentRow]>,
) -> Result<serde_json::Value> {
    let actor = actor_url(config, account.username());
    let note_id = local_status_ap_id(config, account, status);
    let audiences = activitypub_audiences_for_status(db, config, account, status).await?;
    let poll = find_status_poll_by_status_id(db, &status.id).await?;
    let reply_uri = match status.in_reply_to_id.as_deref() {
        Some(reply_id) => find_status_by_id(db, reply_id)
            .await?
            .and_then(|reply| reply.ap_id),
        None => None,
    };
    let attachments = if let Some(attachments) = attachment_override {
        attachments.to_vec()
    } else {
        find_media_attachments_by_status_id(db, &status.id).await?
    };
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
                    "url": media_attachment_url(config, &attachment.id, &attachment.object_key),
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
        note["sensitive"] = serde_json::json!(status.sensitive);
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
        if status.effective_quote_state() == QuoteState::Accepted
            && let Some(stamp_uri) =
                quote_authorization_stamp_uri(db, config, status, quote_uri).await?
        {
            note["quoteAuthorization"] = serde_json::json!(stamp_uri);
        }
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

async fn quote_authorization_stamp_uri(
    db: &D1Database,
    config: &AppConfig,
    quote_status: &StatusRow,
    quote_target_uri: &str,
) -> Result<Option<String>> {
    let Some(target_status) = find_local_status_by_object_uri(db, config, quote_target_uri).await?
    else {
        return Ok(None);
    };
    let Some(target_account) = find_account_by_id(db, &target_status.account_id).await? else {
        return Ok(None);
    };
    Ok(Some(quote_authorization_uri(
        &local_status_ap_id(config, &target_account, &target_status),
        &quote_status.id,
    )))
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

#[cfg(test)]
mod tests {
    use super::*;

    use cfwdon_core::AppConfig;

    fn test_config() -> AppConfig {
        AppConfig {
            instance_domain: "example.com".to_owned(),
            ..AppConfig::default()
        }
    }

    #[test]
    fn activitypub_audiences_rejects_unknown_visibility() {
        assert!(Visibility::parse("not-a-visibility").is_err());
    }

    #[test]
    fn activitypub_audiences_for_visibility_maps_direct_without_followers() {
        let config = test_config();
        let (to, cc) = activitypub_audiences_for_visibility(&config, "alice", Visibility::Direct);
        assert_eq!(to, serde_json::json!([]));
        assert_eq!(cc, serde_json::json!([]));
    }
}
