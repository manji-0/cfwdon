use super::{
    AppConfig, LocalAccount, MediaAttachmentRow, StatusRow, activitypub_datetime_string, actor_url,
    apply_activitypub_poll_fields, classify_media_kind, count_poll_voters,
    extract_account_handles_from_text, extract_hashtags_from_text, find_account_by_id,
    find_account_by_username, find_local_status_by_object_uri, find_media_attachments_by_status_id,
    find_remote_actor_by_username_domain, find_remote_status_by_id, find_status_by_id,
    find_status_poll_by_status_id, is_iso_timestamp_in_past, list_status_poll_options,
    media_attachment_url, media_kind_label, quote_authorization_uri, status_has_active_quote,
    tag_url,
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

fn audience_uri_list(value: serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Array(values) => values
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect(),
        serde_json::Value::String(uri) => vec![uri],
        _ => Vec::new(),
    }
}

fn audience_json(uris: Vec<String>) -> serde_json::Value {
    serde_json::Value::Array(
        uris.into_iter()
            .map(serde_json::Value::String)
            .collect::<Vec<_>>(),
    )
}

fn append_unique_uris(target: &mut Vec<String>, seen: &mut HashSet<String>, uris: Vec<String>) {
    for uri in uris {
        if seen.insert(uri.clone()) {
            target.push(uri);
        }
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

    let (base_to, base_cc) =
        activitypub_audiences_for_visibility(config, account.username(), status.visibility);
    let addressed = collect_addressed_actor_uris(db, config, account, status).await?;
    if addressed.is_empty() {
        return Ok((base_to, base_cc));
    }

    let mut to = audience_uri_list(base_to);
    let cc = audience_uri_list(base_cc);
    let mut seen = to.iter().chain(cc.iter()).cloned().collect::<HashSet<_>>();

    // Mastodon places explicit recipients (mentions / reply parent) on `to`
    // for public and followers-only posts, keeping collection audiences intact.
    append_unique_uris(&mut to, &mut seen, addressed);

    Ok((audience_json(to), audience_json(cc)))
}

async fn collect_addressed_actor_uris(
    db: &D1Database,
    config: &AppConfig,
    author: &LocalAccount,
    status: &StatusRow,
) -> Result<Vec<String>> {
    let mut recipients = Vec::new();
    let mut seen = HashSet::new();

    for handle in extract_account_handles_from_text(&status.text, config) {
        if handle.is_local_to(&config.instance_domain) {
            if let Some(account) = find_account_by_username(db, &handle.username).await?
                && account.id() != author.id()
            {
                let uri = actor_url(config, account.username());
                if seen.insert(uri.clone()) {
                    recipients.push(uri);
                }
            }
            continue;
        }

        if let Some(domain) = handle.domain.as_deref()
            && let Some(actor) =
                find_remote_actor_by_username_domain(db, &handle.username, domain).await?
            && seen.insert(actor.actor_uri.clone())
        {
            recipients.push(actor.actor_uri);
        }
    }

    if let Some(reply_actor_uri) = resolve_reply_actor_uri(db, config, author, status).await?
        && seen.insert(reply_actor_uri.clone())
    {
        recipients.push(reply_actor_uri);
    }

    Ok(recipients)
}

async fn resolve_reply_actor_uri(
    db: &D1Database,
    config: &AppConfig,
    author: &LocalAccount,
    status: &StatusRow,
) -> Result<Option<String>> {
    let Some(reply_id) = status.in_reply_to_id.as_deref() else {
        return Ok(None);
    };

    if let Some(reply) = find_status_by_id(db, reply_id).await?
        && let Some(reply_account) = find_account_by_id(db, &reply.account_id).await?
        && reply_account.id() != author.id()
    {
        return Ok(Some(actor_url(config, reply_account.username())));
    }

    if let Some(reply) = find_remote_status_by_id(db, reply_id).await? {
        return Ok(Some(reply.actor_uri));
    }

    Ok(None)
}

async fn resolve_reply_object_uri(db: &D1Database, status: &StatusRow) -> Result<Option<String>> {
    let Some(reply_id) = status.in_reply_to_id.as_deref() else {
        return Ok(None);
    };

    if let Some(reply) = find_status_by_id(db, reply_id).await?
        && let Some(ap_id) = reply.ap_id
    {
        return Ok(Some(ap_id));
    }

    if let Some(reply) = find_remote_status_by_id(db, reply_id).await? {
        return Ok(Some(reply.object_uri));
    }

    Ok(None)
}

async fn direct_activitypub_audiences(
    db: &D1Database,
    config: &AppConfig,
    author: &LocalAccount,
    status: &StatusRow,
) -> Result<(serde_json::Value, serde_json::Value)> {
    let recipients = collect_addressed_actor_uris(db, config, author, status).await?;
    Ok((audience_json(recipients), serde_json::json!([])))
}

async fn build_activitypub_note_tags(
    db: &D1Database,
    config: &AppConfig,
    author: &LocalAccount,
    status: &StatusRow,
) -> Result<Vec<serde_json::Value>> {
    let mut tags = Vec::new();
    let mut seen_mentions = HashSet::new();

    for handle in extract_account_handles_from_text(&status.text, config) {
        if handle.is_local_to(&config.instance_domain) {
            if let Some(account) = find_account_by_username(db, &handle.username).await? {
                let href = actor_url(config, account.username());
                if seen_mentions.insert(href.clone()) {
                    tags.push(serde_json::json!({
                        "type": "Mention",
                        "href": href,
                        "name": format!("@{}", account.username()),
                    }));
                }
            }
            continue;
        }

        if let Some(domain) = handle.domain.as_deref()
            && let Some(actor) =
                find_remote_actor_by_username_domain(db, &handle.username, domain).await?
            && seen_mentions.insert(actor.actor_uri.clone())
        {
            tags.push(serde_json::json!({
                "type": "Mention",
                "href": actor.actor_uri,
                "name": format!("@{}@{}", handle.username, domain),
            }));
        }
    }

    if let Some(reply_actor_uri) = resolve_reply_actor_uri(db, config, author, status).await?
        && seen_mentions.insert(reply_actor_uri.clone())
    {
        tags.push(serde_json::json!({
            "type": "Mention",
            "href": reply_actor_uri,
        }));
    }

    for tag in extract_hashtags_from_text(&status.text) {
        tags.push(serde_json::json!({
            "type": "Hashtag",
            "href": tag_url(config, &tag),
            "name": format!("#{tag}"),
        }));
    }

    Ok(tags)
}

pub(crate) fn quote_context_mapping() -> serde_json::Value {
    serde_json::json!({
        "_misskey_quote": {
            "@id": "https://misskey-hub.net/ns#_misskey_quote",
            "@type": "@id"
        },
        "quoteUri": {
            "@id": "http://fedibird.com/ns#quoteUri",
            "@type": "@id"
        },
        "quoteUrl": {
            "@id": "https://www.w3.org/ns/activitystreams#quoteUrl",
            "@type": "@id"
        },
        "quoteAuthorization": {
            "@id": "https://w3id.org/fep/044f#quoteAuthorization",
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
    let reply_uri = resolve_reply_object_uri(db, status).await?;
    let tags = build_activitypub_note_tags(db, config, account, status).await?;
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
        "published": activitypub_datetime_string(&status.created_at),
        "to": audiences.0,
        "cc": audiences.1,
        "tag": tags,
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

    #[test]
    fn quote_context_mapping_declares_quote_iri_terms() {
        let mapping = quote_context_mapping();
        assert_eq!(
            mapping["_misskey_quote"]["@id"],
            serde_json::json!("https://misskey-hub.net/ns#_misskey_quote")
        );
        assert_eq!(
            mapping["quoteUri"],
            serde_json::json!({
                "@id": "http://fedibird.com/ns#quoteUri",
                "@type": "@id"
            })
        );
        assert_eq!(
            mapping["quoteUrl"],
            serde_json::json!({
                "@id": "https://www.w3.org/ns/activitystreams#quoteUrl",
                "@type": "@id"
            })
        );
        assert_eq!(
            mapping["quoteAuthorization"],
            serde_json::json!({
                "@id": "https://w3id.org/fep/044f#quoteAuthorization",
                "@type": "@id"
            })
        );
    }

    #[test]
    fn note_context_includes_quote_terms_when_quote_present() {
        let context = note_context(true, false);
        let entries = context.as_array().expect("quote note context is an array");
        assert!(entries.iter().any(|entry| {
            entry.get("quoteUri").is_some()
                && entry.get("quoteUrl").is_some()
                && entry.get("quoteAuthorization").is_some()
                && entry.get("_misskey_quote").is_some()
        }));
    }
}
