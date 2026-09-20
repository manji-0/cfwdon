use url::Url;

pub(crate) fn first_url_from_text(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        let trimmed = token
            .trim_matches(|ch: char| {
                matches!(
                    ch,
                    '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | '.'
                )
            })
            .trim();
        (!trimmed.is_empty()
            && (trimmed.starts_with("https://") || trimmed.starts_with("http://"))
            && Url::parse(trimmed).is_ok())
        .then(|| trimmed.to_owned())
    })
}

pub(super) fn collapsed_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn display_provider_name(parsed: &Url) -> String {
    parsed
        .host_str()
        .unwrap_or_default()
        .trim_start_matches("www.")
        .to_owned()
}

pub(super) fn provider_url(parsed: &Url) -> String {
    let Some(host) = parsed.host_str() else {
        return String::new();
    };
    match parsed.port() {
        Some(port) => format!("{}://{}:{port}", parsed.scheme(), host),
        None => format!("{}://{}", parsed.scheme(), host),
    }
}

pub(super) fn strip_common_document_extension(segment: &str) -> &str {
    segment
        .strip_suffix(".html")
        .or_else(|| segment.strip_suffix(".htm"))
        .or_else(|| segment.strip_suffix(".php"))
        .or_else(|| segment.strip_suffix(".asp"))
        .or_else(|| segment.strip_suffix(".aspx"))
        .unwrap_or(segment)
}

pub(super) fn display_title_from_url(parsed: &Url, provider_name: &str) -> String {
    let Some(last_segment) = parsed
        .path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
    else {
        return provider_name.to_owned();
    };
    let decoded = urlencoding::decode(last_segment)
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| last_segment.to_owned());
    let simplified = strip_common_document_extension(&decoded)
        .replace(['-', '_', '+'], " ")
        .replace("%20", " ");
    let collapsed = collapsed_whitespace(&simplified);
    if collapsed.is_empty() {
        provider_name.to_owned()
    } else {
        collapsed
    }
}

pub(super) fn status_card_description_from_text(text: &str, url: &str) -> String {
    let description = text
        .split_whitespace()
        .filter(|token| {
            token.trim_matches(|ch: char| {
                matches!(
                    ch,
                    '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | '.'
                )
            }) != url
        })
        .collect::<Vec<_>>()
        .join(" ");
    let collapsed = collapsed_whitespace(&description);
    if collapsed.chars().count() <= 300 {
        return collapsed;
    }
    let truncated = collapsed.chars().take(300).collect::<String>();
    format!("{}…", truncated.trim_end())
}

pub(crate) fn build_status_card_value(text: &str) -> Option<serde_json::Value> {
    let url = first_url_from_text(text)?;
    let parsed = Url::parse(&url).ok()?;
    let provider_name = display_provider_name(&parsed);
    let provider_url = provider_url(&parsed);
    let title = display_title_from_url(&parsed, &provider_name);
    let description = status_card_description_from_text(text, &url);
    Some(serde_json::json!({
        "url": url,
        "title": title,
        "description": description,
        "type": "link",
        "authors": [],
        "author_name": "",
        "author_url": "",
        "provider_name": provider_name,
        "provider_url": provider_url,
        "html": "",
        "width": 0,
        "height": 0,
        "image": serde_json::Value::Null,
        "embed_url": "",
        "blurhash": serde_json::Value::Null,
    }))
}

pub(super) fn remote_status_attachment_card_candidate(
    attachments: &[crate::RemoteStatusAttachmentRow],
) -> Option<&crate::RemoteStatusAttachmentRow> {
    attachments.iter().find(|attachment| {
        if Url::parse(&attachment.remote_url).is_err() {
            return false;
        }

        if attachment
            .preview_url
            .as_deref()
            .is_some_and(|preview| preview != attachment.remote_url && Url::parse(preview).is_ok())
        {
            return true;
        }

        let content_type = attachment
            .content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim();
        matches!(content_type, "text/html" | "application/xhtml+xml")
            || crate::classify_media_kind(content_type).is_none()
    })
}

pub(crate) fn build_remote_status_card_value(
    text: &str,
    attachments: &[crate::RemoteStatusAttachmentRow],
) -> Option<serde_json::Value> {
    let mut card = build_status_card_value(text)?;
    let Some(attachment) = remote_status_attachment_card_candidate(attachments) else {
        return Some(card);
    };

    let parsed = Url::parse(&attachment.remote_url).ok()?;
    let provider_name = display_provider_name(&parsed);
    card["url"] = serde_json::json!(attachment.remote_url);
    card["provider_name"] = serde_json::json!(provider_name.clone());
    card["provider_url"] = serde_json::json!(provider_url(&parsed));
    card["title"] = serde_json::json!(
        attachment
            .description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(collapsed_whitespace)
            .unwrap_or_else(|| display_title_from_url(&parsed, &provider_name))
    );
    if let Some(preview_url) = attachment
        .preview_url
        .as_deref()
        .filter(|preview| *preview != attachment.remote_url)
        .filter(|preview| Url::parse(preview).is_ok())
    {
        card["image"] = serde_json::json!(preview_url);
    }
    if let Some(width) = attachment.width {
        card["width"] = serde_json::json!(width);
    }
    if let Some(height) = attachment.height {
        card["height"] = serde_json::json!(height);
    }
    if let Some(blurhash) = attachment
        .blurhash
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        card["blurhash"] = serde_json::json!(blurhash);
    }
    Some(card)
}
