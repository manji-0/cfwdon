use super::preview_card::collapsed_whitespace;
use crate::statuses::{Error, Result, parse_remote_http_url};

pub(super) const REMOTE_PREVIEW_HTML_LIMIT: usize = 65_536;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct HtmlPreviewMetadata {
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) provider_name: Option<String>,
    pub(crate) image: Option<String>,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
}

pub(super) fn html_unescape(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

pub(super) fn html_attr_value(tag: &str, attr_name: &str) -> Option<String> {
    let lower_tag = tag.to_ascii_lowercase();
    let needle = format!("{attr_name}=");
    let index = lower_tag.find(&needle)?;
    let mut value = tag[index + needle.len()..].trim_start().chars();
    let quote = value.next()?;
    if !matches!(quote, '"' | '\'') {
        return None;
    }
    let rest = &tag[index + needle.len() + quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_owned())
}

pub(super) fn find_html_tag<'a>(html: &'a str, tag_name: &str) -> Vec<&'a str> {
    let needle = format!("<{tag_name}");
    html.match_indices(&needle)
        .filter_map(|(index, _)| {
            html[index..]
                .find('>')
                .map(|end| &html[index..=index + end])
        })
        .collect()
}

pub(super) fn find_meta_content(html: &str, attr_name: &str, attr_value: &str) -> Option<String> {
    find_html_tag(html, "meta")
        .into_iter()
        .find(|tag| {
            html_attr_value(tag, attr_name)
                .map(|value| value.eq_ignore_ascii_case(attr_value))
                .unwrap_or(false)
        })
        .and_then(|tag| html_attr_value(tag, "content"))
        .map(|value| collapsed_whitespace(&html_unescape(&value)))
        .filter(|value| !value.is_empty())
}

pub(super) fn find_link_href(html: &str, rel_value: &str) -> Option<String> {
    find_html_tag(html, "link")
        .into_iter()
        .find(|tag| {
            html_attr_value(tag, "rel")
                .map(|value| value.eq_ignore_ascii_case(rel_value))
                .unwrap_or(false)
        })
        .and_then(|tag| html_attr_value(tag, "href"))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub(super) fn find_title_tag_content(html: &str) -> Option<String> {
    let lower_html = html.to_ascii_lowercase();
    let start = lower_html.find("<title")?;
    let title_open_end = html[start..].find('>')? + start + 1;
    let end = lower_html[title_open_end..].find("</title>")? + title_open_end;
    let content = collapsed_whitespace(&html_unescape(html[title_open_end..end].trim()));
    (!content.is_empty()).then_some(content)
}

pub(crate) fn extract_html_preview_metadata(html: &str) -> HtmlPreviewMetadata {
    let head = html
        .chars()
        .take(REMOTE_PREVIEW_HTML_LIMIT)
        .collect::<String>();
    HtmlPreviewMetadata {
        title: find_meta_content(&head, "property", "og:title")
            .or_else(|| find_meta_content(&head, "name", "twitter:title"))
            .or_else(|| find_title_tag_content(&head)),
        description: find_meta_content(&head, "property", "og:description")
            .or_else(|| find_meta_content(&head, "name", "description"))
            .or_else(|| find_meta_content(&head, "name", "twitter:description")),
        provider_name: find_meta_content(&head, "property", "og:site_name")
            .or_else(|| find_meta_content(&head, "name", "application-name")),
        image: find_meta_content(&head, "property", "og:image")
            .or_else(|| find_meta_content(&head, "name", "twitter:image"))
            .or_else(|| find_link_href(&head, "image_src")),
        width: find_meta_content(&head, "property", "og:image:width")
            .and_then(|value| value.parse::<u32>().ok()),
        height: find_meta_content(&head, "property", "og:image:height")
            .and_then(|value| value.parse::<u32>().ok()),
    }
}

pub(crate) fn apply_html_preview_metadata(
    card: &mut serde_json::Value,
    metadata: &HtmlPreviewMetadata,
) {
    if let Some(title) = metadata.title.as_deref() {
        card["title"] = serde_json::json!(title);
    }
    if let Some(description) = metadata.description.as_deref() {
        card["description"] = serde_json::json!(description);
    }
    if let Some(provider_name) = metadata.provider_name.as_deref() {
        card["provider_name"] = serde_json::json!(provider_name);
    }
    if let Some(image) = metadata.image.as_deref() {
        card["image"] = serde_json::json!(image);
    }
    if let Some(width) = metadata.width {
        card["width"] = serde_json::json!(width);
    }
    if let Some(height) = metadata.height {
        card["height"] = serde_json::json!(height);
    }
}

pub(super) async fn fetch_remote_preview_html(url: &str) -> Result<String> {
    let parsed = parse_remote_http_url(url)?;
    crate::federation::validate_remote_fetch_url(&parsed).await?;

    let headers = worker::Headers::new();
    headers.set(
        "Accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.1",
    )?;

    let mut init = worker::RequestInit::new();
    init.with_method(worker::Method::Get).with_headers(headers);
    let request = worker::Request::new_with_init(parsed.as_str(), &init)?;
    let mut response = worker::Fetch::Request(request).send().await?;
    if response.status_code() / 100 != 2 {
        return Err(Error::RustError(format!(
            "failed to fetch remote preview document {}: HTTP {}",
            url,
            response.status_code()
        )));
    }

    response.text().await
}

pub(crate) async fn enrich_card_with_remote_preview(card: &mut serde_json::Value) -> Result<bool> {
    let Some(url) = card
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(false);
    };

    let html = match fetch_remote_preview_html(url).await {
        Ok(html) => html,
        Err(_) => return Ok(false),
    };
    let metadata = extract_html_preview_metadata(&html);
    if metadata == HtmlPreviewMetadata::default() {
        return Ok(false);
    }
    apply_html_preview_metadata(card, &metadata);
    Ok(true)
}
