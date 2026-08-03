use super::{
    CACHE_TTL_FEDERATION, CACHE_TTL_STATUS_API, Error, MastodonAccountResponse, Request, Response,
    Result, RouteContext, build_activitypub_note, build_finished_context_async_refresh_header,
    build_local_status_context, build_local_status_response, build_remote_status_context,
    build_remote_status_response, cache_public_json_response, cache_public_response_with_options,
    cache_status_api_response, cached_status_api_response, find_account_by_id,
    find_account_by_username, find_authenticated_local_account, find_local_status_by_object_uri,
    find_remote_actor_by_actor_uri, find_remote_status_attachments_by_status_id,
    find_remote_status_by_id, find_remote_status_by_url_or_object_uri, find_status_by_id,
    find_visible_local_status_response_subject, is_public_activitypub_visibility,
    list_local_favourite_account_ids_for_remote_status,
    list_local_favourite_account_ids_for_status, list_local_reblog_account_ids_for_remote_status,
    list_local_reblog_account_ids_for_status, list_remote_favourite_actor_uris_for_status,
    list_remote_reblog_actor_uris_for_status, list_remote_status_edit_snapshots,
    load_account_stats, load_config, load_local_status_response_preload,
    load_remote_status_updated_at, load_visible_local_status_response_subject,
    remote_account_rest_id, resolve_local_status_response_subject, status_id_from_context,
    strip_html_tags, timestamp_to_mastodon_iso8601,
};
use serde::{Deserialize, Serialize};
use url::Url;

const REMOTE_PREVIEW_HTML_LIMIT: usize = 65_536;

#[derive(Debug, Default, Deserialize)]
struct StatusInteractionAccountsQuery {
    limit: Option<u32>,
}

#[derive(Clone, Copy)]
enum StatusInteractionKind {
    Reblogged,
    Favourited,
}

pub(crate) enum ResolvedStatus {
    Local(crate::StatusRow),
    Remote(crate::RemoteStatusRow),
}

enum LoadedStatusApiSubject {
    Local(super::LoadedLocalStatusResponseSubject),
    Remote {
        status: crate::RemoteStatusRow,
        actor: crate::RemoteActorRow,
    },
}

struct StatusDetailBaseContext {
    config: cfwdon_core::AppConfig,
    session: crate::D1RequestSession,
    db: crate::D1Database,
    status_id: String,
}

struct StatusDetailRequestContext {
    base: StatusDetailBaseContext,
    viewer: Option<crate::LocalAccount>,
}

#[derive(Debug, Serialize)]
struct StatusSourceResponse {
    id: String,
    text: String,
    spoiler_text: String,
}

pub(crate) fn normalize_status_history_entry(mut value: serde_json::Value) -> serde_json::Value {
    let content = value
        .get("content")
        .cloned()
        .unwrap_or_else(|| serde_json::json!(""));
    let spoiler_text = value
        .get("spoiler_text")
        .cloned()
        .unwrap_or_else(|| serde_json::json!(""));
    let sensitive = value
        .get("sensitive")
        .cloned()
        .unwrap_or(serde_json::Value::Bool(false));
    let created_at = value
        .get("created_at")
        .and_then(serde_json::Value::as_str)
        .map(timestamp_to_mastodon_iso8601)
        .unwrap_or_default();
    let account = value
        .get("account")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let media_attachments = match value.get_mut("media_attachments") {
        Some(serde_json::Value::Array(items)) => serde_json::Value::Array(items.clone()),
        _ => serde_json::json!([]),
    };
    let emojis = match value.get_mut("emojis") {
        Some(serde_json::Value::Array(items)) => serde_json::Value::Array(items.clone()),
        _ => serde_json::json!([]),
    };
    let poll = value
        .get("poll")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let quote = value
        .get("quote")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    serde_json::json!({
        "content": content,
        "spoiler_text": spoiler_text,
        "sensitive": sensitive,
        "created_at": created_at,
        "account": account,
        "media_attachments": media_attachments,
        "emojis": emojis,
        "poll": poll,
        "quote": quote,
    })
}

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

fn collapsed_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn display_provider_name(parsed: &Url) -> String {
    parsed
        .host_str()
        .unwrap_or_default()
        .trim_start_matches("www.")
        .to_owned()
}

fn provider_url(parsed: &Url) -> String {
    let Some(host) = parsed.host_str() else {
        return String::new();
    };
    match parsed.port() {
        Some(port) => format!("{}://{}:{port}", parsed.scheme(), host),
        None => format!("{}://{}", parsed.scheme(), host),
    }
}

fn strip_common_document_extension(segment: &str) -> &str {
    segment
        .strip_suffix(".html")
        .or_else(|| segment.strip_suffix(".htm"))
        .or_else(|| segment.strip_suffix(".php"))
        .or_else(|| segment.strip_suffix(".asp"))
        .or_else(|| segment.strip_suffix(".aspx"))
        .unwrap_or(segment)
}

fn display_title_from_url(parsed: &Url, provider_name: &str) -> String {
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

fn status_card_description_from_text(text: &str, url: &str) -> String {
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

fn remote_status_attachment_card_candidate(
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

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct HtmlPreviewMetadata {
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) provider_name: Option<String>,
    pub(crate) image: Option<String>,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
}

fn html_unescape(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn html_attr_value(tag: &str, attr_name: &str) -> Option<String> {
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

fn find_html_tag<'a>(html: &'a str, tag_name: &str) -> Vec<&'a str> {
    let needle = format!("<{tag_name}");
    html.match_indices(&needle)
        .filter_map(|(index, _)| {
            html[index..]
                .find('>')
                .map(|end| &html[index..=index + end])
        })
        .collect()
}

fn find_meta_content(html: &str, attr_name: &str, attr_value: &str) -> Option<String> {
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

fn find_link_href(html: &str, rel_value: &str) -> Option<String> {
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

fn find_title_tag_content(html: &str) -> Option<String> {
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

async fn fetch_remote_preview_html(url: &str) -> Result<String> {
    let parsed = super::parse_remote_http_url(url)?;
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

pub(crate) async fn resolve_status_reference(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    id: &str,
) -> Result<Option<ResolvedStatus>> {
    let raw_id = id.trim();
    if raw_id.is_empty() {
        return Ok(None);
    }

    if let Some(status) = find_status_by_id(db, raw_id).await? {
        return Ok(Some(ResolvedStatus::Local(status)));
    }
    if let Some(status) = find_remote_status_by_id(db, raw_id).await? {
        return Ok(Some(ResolvedStatus::Remote(status)));
    }

    let decoded_id = urlencoding::decode(raw_id)
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| raw_id.to_owned());
    if let Some(status) = find_local_status_by_object_uri(db, config, &decoded_id).await? {
        return Ok(Some(ResolvedStatus::Local(status)));
    }
    if let Some(status) = find_remote_status_by_url_or_object_uri(db, &decoded_id).await? {
        return Ok(Some(ResolvedStatus::Remote(status)));
    }

    Ok(None)
}

fn resolve_status_detail_base_context(
    req: &Request,
    ctx: &RouteContext<()>,
) -> Result<Option<StatusDetailBaseContext>> {
    let status_id = match status_id_from_context(ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Ok(None),
    };
    let config = load_config(ctx);
    let (session, db) = crate::open_bound_request_session(ctx, &config, req)?;
    Ok(Some(StatusDetailBaseContext {
        config,
        session,
        db,
        status_id,
    }))
}

async fn resolve_status_detail_request_context(
    req: &Request,
    ctx: &RouteContext<()>,
) -> Result<Option<StatusDetailRequestContext>> {
    let Some(base) = resolve_status_detail_base_context(req, ctx)? else {
        return Ok(None);
    };
    let viewer = find_authenticated_local_account(req, &base.db, &base.config).await?;
    Ok(Some(StatusDetailRequestContext { base, viewer }))
}

async fn build_local_interaction_account_responses(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    account_ids: &[String],
) -> Result<Vec<MastodonAccountResponse>> {
    let mut responses = Vec::new();

    for account_id in account_ids {
        let Some(account) = find_account_by_id(db, account_id).await? else {
            continue;
        };
        let stats = load_account_stats(db, account.id()).await?;
        responses.push(MastodonAccountResponse::from_account_with_stats(
            &account, config, &stats,
        ));
    }

    Ok(responses)
}

async fn build_remote_interaction_account_response(
    db: &crate::D1Database,
    actor_uri: &str,
) -> Result<Option<MastodonAccountResponse>> {
    let status_summary = crate::load_remote_actor_status_summary(db, actor_uri).await?;

    if let Some(actor) = find_remote_actor_by_actor_uri(db, actor_uri).await? {
        let mut response = MastodonAccountResponse::from_remote_actor(&actor);
        response.statuses_count = status_summary.statuses_count;
        response.last_status_at = status_summary.last_status_at.clone();
        return Ok(Some(response));
    }

    let parsed = match Url::parse(actor_uri) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(None),
    };
    let Some(domain) = parsed.host_str().map(str::to_owned) else {
        return Ok(None);
    };
    let Some(username) = parsed
        .path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
        .map(|segment| segment.trim_start_matches('@').to_owned())
        .filter(|segment| !segment.is_empty())
    else {
        return Ok(None);
    };

    Ok(Some(MastodonAccountResponse {
        id: remote_account_rest_id(actor_uri),
        username: username.clone(),
        acct: format!("{username}@{domain}"),
        uri: actor_uri.to_owned(),
        display_name: username.clone(),
        locked: false,
        bot: false,
        group: false,
        discoverable: true,
        indexable: true,
        noindex: None,
        hide_collections: None,
        show_media: None,
        show_media_replies: None,
        show_featured: None,
        last_status_at: status_summary.last_status_at,
        created_at: String::new(),
        note: String::new(),
        url: actor_uri.to_owned(),
        avatar: String::new(),
        avatar_static: String::new(),
        avatar_description: String::new(),
        header: String::new(),
        header_static: String::new(),
        header_description: String::new(),
        emojis: Vec::new(),
        fields: Vec::new(),
        roles: None,
        feature_approval: serde_json::json!({
            "automatic": [],
            "manual": [],
            "current_user": "missing",
        }),
        followers_count: 0,
        following_count: 0,
        statuses_count: status_summary.statuses_count,
        source: None,
        role: None,
    }))
}

async fn build_remote_interaction_account_responses(
    db: &crate::D1Database,
    actor_uris: &[String],
) -> Result<Vec<MastodonAccountResponse>> {
    let mut responses = Vec::new();

    for actor_uri in actor_uris {
        if let Some(response) = build_remote_interaction_account_response(db, actor_uri).await? {
            responses.push(response);
        }
    }

    Ok(responses)
}

async fn status_interaction_accounts_response(
    req: Request,
    ctx: RouteContext<()>,
    kind: StatusInteractionKind,
) -> Result<Response> {
    let Some(detail) = resolve_status_detail_base_context(&req, &ctx)? else {
        return Response::error("missing status id route parameter", 400);
    };
    let Some(status) =
        resolve_status_reference(&detail.db, &detail.config, &detail.status_id).await?
    else {
        return Response::error("status not found", 404);
    };
    let query: StatusInteractionAccountsQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).min(80);

    let mut responses = match status {
        ResolvedStatus::Local(status) => {
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
                return Response::error("status not found", 404);
            }
            let local_accounts = match kind {
                StatusInteractionKind::Reblogged => {
                    list_local_reblog_account_ids_for_status(&detail.db, &status.id, limit).await?
                }
                StatusInteractionKind::Favourited => {
                    list_local_favourite_account_ids_for_status(&detail.db, &status.id, limit)
                        .await?
                }
            };
            let mut responses = build_local_interaction_account_responses(
                &detail.db,
                &detail.config,
                &local_accounts,
            )
            .await?;
            if responses.len() < limit as usize {
                let remaining = limit.saturating_sub(responses.len() as u32);
                let remote_actor_uris = match kind {
                    StatusInteractionKind::Reblogged => {
                        list_remote_reblog_actor_uris_for_status(&detail.db, &status.id, remaining)
                            .await?
                    }
                    StatusInteractionKind::Favourited => {
                        list_remote_favourite_actor_uris_for_status(
                            &detail.db, &status.id, remaining,
                        )
                        .await?
                    }
                };
                responses.extend(
                    build_remote_interaction_account_responses(&detail.db, &remote_actor_uris)
                        .await?,
                );
            }
            responses
        }
        ResolvedStatus::Remote(status) => {
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
                return Response::error("status not found", 404);
            }
            let local_accounts = match kind {
                StatusInteractionKind::Reblogged => {
                    list_local_reblog_account_ids_for_remote_status(&detail.db, &status.id, limit)
                        .await?
                }
                StatusInteractionKind::Favourited => {
                    list_local_favourite_account_ids_for_remote_status(
                        &detail.db, &status.id, limit,
                    )
                    .await?
                }
            };
            build_local_interaction_account_responses(&detail.db, &detail.config, &local_accounts)
                .await?
        }
    };
    responses.truncate(limit as usize);
    crate::with_d1_bookmark(Response::from_json(&responses)?, &detail.session)
}

pub(crate) async fn status_reblogged_by_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    status_interaction_accounts_response(req, ctx, StatusInteractionKind::Reblogged).await
}

pub(crate) async fn status_favourited_by_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    status_interaction_accounts_response(req, ctx, StatusInteractionKind::Favourited).await
}

pub(crate) async fn status_object_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().trim_start_matches('@').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing status id route parameter".to_owned()))?;

    let db = crate::bind_request_d1(&ctx, &config)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let Some(status) = find_status_by_id(&db, &status_id).await? else {
        return Response::error("status not found", 404);
    };
    if status.account_id != account.id() {
        return Response::error("status not found", 404);
    }
    if !is_public_activitypub_visibility(status.visibility.as_str()) {
        return Response::error("status not found", 404);
    }

    if status_object_prefers_html(&req)? {
        let preload = load_local_status_response_preload(&db, &status).await?;
        return cache_public_response_with_options(
            status_object_html_response(&config, &account, &status, &preload.media)?,
            CACHE_TTL_FEDERATION,
            None,
            &[
                ("Vary", "Accept"),
                ("Cache-Tag", &format!("status-{status_id}")),
            ],
        );
    }

    let note = build_activitypub_note(&db, &config, &account, &status, true, None).await?;
    cache_public_json_response(
        &note,
        "application/activity+json; charset=utf-8",
        CACHE_TTL_FEDERATION,
        &[
            ("Vary", "Accept"),
            ("Cache-Tag", &format!("status-{status_id}")),
        ],
    )
}

pub(crate) async fn status_quote_authorization_object_response(
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;
    let target_status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing status id route parameter".to_owned()))?;
    let authorization_key = ctx
        .param("authorization_key")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing authorization key route parameter".to_owned()))?;

    let db = crate::bind_request_d1(&ctx, &config)?;
    let Some(target_account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let Some(target_status) = find_status_by_id(&db, &target_status_id).await? else {
        return Response::error("status not found", 404);
    };
    if target_status.account_id != target_account.id() {
        return Response::error("status not found", 404);
    }
    if !is_public_activitypub_visibility(target_status.visibility.as_str()) {
        return Response::error("status not found", 404);
    }

    let target_uri = crate::local_status_target_uri(&target_status);
    let (interacting_object_uri, quote_state) = if let Some(quote_status) =
        find_status_by_id(&db, &authorization_key).await?
    {
        if quote_status.quote_of_uri.as_deref() != Some(target_uri.as_str()) {
            return Response::error("quote authorization not found", 404);
        }
        (
            crate::local_status_target_uri(&quote_status),
            quote_status.effective_quote_state(),
        )
    } else if let Some(quote_status) = find_remote_status_by_id(&db, &authorization_key).await? {
        if quote_status.quote_of_uri.as_deref() != Some(target_uri.as_str()) {
            return Response::error("quote authorization not found", 404);
        }
        (
            quote_status.object_uri.clone(),
            quote_status.effective_quote_state(),
        )
    } else {
        return Response::error("quote authorization not found", 404);
    };

    if quote_state != cfwdon_domain::QuoteState::Accepted {
        return Response::error("quote authorization not found", 404);
    }

    let document = crate::build_quote_authorization_object(
        &config,
        &target_account,
        &interacting_object_uri,
        &target_uri,
        &authorization_key,
    );
    cache_public_json_response(
        &document,
        "application/activity+json; charset=utf-8",
        CACHE_TTL_FEDERATION,
        &[("Cache-Tag", &format!("status-{target_status_id}"))],
    )
}

pub(crate) fn status_object_prefers_html(req: &Request) -> Result<bool> {
    let accept = req.headers().get("Accept")?.unwrap_or_default();
    let accept = accept.to_ascii_lowercase();
    Ok(accept.contains("text/html")
        && !accept.contains("application/activity+json")
        && !accept.contains("application/ld+json"))
}

fn status_object_html_response(
    config: &crate::AppConfig,
    account: &crate::LocalAccount,
    status: &crate::StatusRow,
    attachments: &[crate::MediaAttachmentRow],
) -> Result<Response> {
    let title_text = strip_html_tags(&status.content_html);
    let fallback_title;
    let title_source = if title_text.is_empty() {
        fallback_title = format!("@{}", account.username());
        fallback_title.as_str()
    } else {
        &title_text
    };
    let title = crate::escape_html(title_source);
    let account_name = crate::escape_html(account.acct());
    let published = crate::escape_html(&status.created_at);
    let status_url = crate::local_status_ap_id(config, account, status);
    let oembed_link = status_oembed_discovery_link(config, &status_url);
    let media_html = attachments
        .iter()
        .filter(|attachment| {
            crate::classify_media_kind(&attachment.content_type) == Some(crate::MediaKind::Image)
        })
        .map(|attachment| {
            let src = crate::escape_html(&crate::media_attachment_url(
                config,
                &attachment.id,
                &attachment.object_key,
            ));
            let alt = crate::escape_html(&attachment.description);
            format!("<img src=\"{src}\" alt=\"{alt}\" loading=\"lazy\">")
        })
        .collect::<Vec<_>>()
        .join("");
    let html = format!(
        "<!doctype html><html lang=\"ja\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title}</title>{oembed_link}<style>body{{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:0;background:#0f1115;color:#f4f4f5}}main{{max-width:680px;margin:0 auto;padding:24px}}article{{border:1px solid #2b2f36;border-radius:8px;padding:20px;background:#171a21}}.account{{color:#a1a1aa;margin-bottom:12px}}.content{{font-size:18px;line-height:1.6}}.media{{display:grid;gap:12px;margin-top:16px}}img{{max-width:100%;border-radius:8px}}time{{display:block;color:#a1a1aa;margin-top:16px;font-size:14px}}</style></head><body><main><article><div class=\"account\">{account_name}</div><div class=\"content\">{content}</div><div class=\"media\">{media_html}</div><time>{published}</time></article></main></body></html>",
        content = status.content_html,
    );
    let mut response = Response::from_body(worker::ResponseBody::Body(html.into_bytes()))?;
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    Ok(response)
}

fn status_oembed_discovery_link(config: &crate::AppConfig, status_url: &str) -> String {
    let href = format!(
        "{}/api/oembed?url={}",
        crate::instance_base_url(config),
        urlencoding::encode(status_url)
    );
    format!(
        "<link rel=\"alternate\" type=\"application/json+oembed\" href=\"{}\">",
        crate::escape_html(&href)
    )
}

pub(crate) async fn status_card_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let Some(detail) = resolve_status_detail_base_context(&req, &ctx)? else {
        return Response::error("missing status id route parameter", 400);
    };

    let Some(status) =
        resolve_status_reference(&detail.db, &detail.config, &detail.status_id).await?
    else {
        return Response::error("status not found", 404);
    };

    let mut card = match status {
        ResolvedStatus::Local(status) => {
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
                return Response::error("status not found", 404);
            }
            build_status_card_value(&status.text)
        }
        ResolvedStatus::Remote(status) => {
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
                return Response::error("status not found", 404);
            }
            let attachments =
                find_remote_status_attachments_by_status_id(&detail.db, &status.id).await?;
            build_remote_status_card_value(&status.plain_text(), &attachments)
        }
    }
    .unwrap_or(serde_json::Value::Null);
    let _ = enrich_card_with_remote_preview(&mut card).await;

    crate::with_d1_bookmark(Response::from_json(&card)?, &detail.session)
}

pub(crate) async fn status_api_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let Some(detail) = resolve_status_detail_request_context(&req, &ctx).await? else {
        return Response::error("missing status id route parameter", 400);
    };
    if detail.viewer.is_none()
        && let Some(response) = cached_status_api_response(&ctx, &detail.base.status_id).await?
    {
        return Ok(response);
    }
    let Some(status) =
        resolve_status_reference(&detail.base.db, &detail.base.config, &detail.base.status_id)
            .await?
    else {
        return Response::error("status not found", 404);
    };

    let Some(subject) = load_status_api_subject(
        &detail.base.db,
        &detail.base.config,
        detail.viewer.as_ref(),
        status,
    )
    .await?
    else {
        return Response::error("status not found", 404);
    };
    let response = build_status_api_document(
        &detail.base.db,
        &detail.base.config,
        detail.viewer.as_ref(),
        subject,
    )
    .await?;
    if detail.viewer.is_none() {
        cache_status_api_response(&ctx, &detail.base.status_id, &response).await?;
        let cached = cache_public_json_response(
            &response,
            "application/json; charset=utf-8",
            CACHE_TTL_STATUS_API,
            &[("Cache-Tag", &format!("status-{}", detail.base.status_id))],
        )?;
        return crate::with_d1_bookmark(cached, &detail.base.session);
    }
    crate::with_d1_bookmark(Response::from_json(&response)?, &detail.base.session)
}

async fn load_status_api_subject(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    status: ResolvedStatus,
) -> Result<Option<LoadedStatusApiSubject>> {
    match status {
        ResolvedStatus::Local(status) => {
            load_local_status_api_subject(db, config, viewer, status).await
        }
        ResolvedStatus::Remote(status) => load_remote_status_api_subject(db, status).await,
    }
}

async fn build_status_api_document(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    subject: LoadedStatusApiSubject,
) -> Result<crate::MastodonStatusResponse> {
    match subject {
        LoadedStatusApiSubject::Local(subject) => {
            let super::LoadedLocalStatusResponseSubject {
                status,
                account,
                preload:
                    super::LocalStatusResponsePreload {
                        in_reply_to_account_id,
                        media,
                    },
            } = subject;
            build_local_status_response(
                db,
                config,
                viewer,
                &status,
                &account,
                in_reply_to_account_id,
                media,
            )
            .await
        }
        LoadedStatusApiSubject::Remote { status, actor } => {
            build_remote_status_response(db, config, viewer, &status, &actor).await
        }
    }
}

async fn load_local_status_api_subject(
    db: &crate::D1Database,
    _config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    status: crate::StatusRow,
) -> Result<Option<LoadedStatusApiSubject>> {
    match resolve_local_status_response_subject(db, viewer, status).await? {
        Some(super::ResolvedLocalStatusResponseSubject::Loaded(subject)) => {
            Ok(Some(LoadedStatusApiSubject::Local(subject)))
        }
        Some(super::ResolvedLocalStatusResponseSubject::Hidden) | None => Ok(None),
    }
}

async fn load_remote_status_api_subject(
    db: &crate::D1Database,
    status: crate::RemoteStatusRow,
) -> Result<Option<LoadedStatusApiSubject>> {
    if !is_public_activitypub_visibility(status.visibility.as_str()) {
        return Ok(None);
    }
    let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await? else {
        return Ok(None);
    };
    Ok(Some(LoadedStatusApiSubject::Remote { status, actor }))
}

async fn context_response_with_async_refresh<T: Serialize>(
    db: &crate::D1Database,
    status_id: &str,
    viewer_present: bool,
    context: &T,
) -> Result<Response> {
    let mut response = Response::from_json(context)?;
    if viewer_present {
        let header = build_finished_context_async_refresh_header(db, status_id).await?;
        response
            .headers_mut()
            .set("Mastodon-Async-Refresh", &header)?;
    }
    Ok(response)
}

fn status_history_response_from_parts(
    response: crate::MastodonStatusResponse,
    created_at: String,
    snapshots: Vec<serde_json::Value>,
) -> Result<Response> {
    let mut current_revision = serde_json::to_value(response).unwrap_or(serde_json::json!({}));
    current_revision["created_at"] = serde_json::json!(created_at);
    let mut history = vec![normalize_status_history_entry(current_revision)];
    history.extend(snapshots);
    Response::from_json(&history)
}

pub(crate) async fn status_source_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(detail) = resolve_status_detail_request_context(&req, &ctx).await? else {
        return Response::error("missing status id route parameter", 400);
    };
    let Some(viewer) = detail.viewer else {
        return Response::error("Auth0 authentication required", 401);
    };
    let Some(status) =
        resolve_status_reference(&detail.base.db, &detail.base.config, &detail.base.status_id)
            .await?
    else {
        return Response::error("status not found", 404);
    };
    let ResolvedStatus::Local(status) = status else {
        return Response::error("status source is only available for local statuses", 403);
    };
    let Some(subject) =
        load_visible_local_status_response_subject(&detail.base.db, Some(&viewer), status).await?
    else {
        return Response::error("status not found", 404);
    };
    let super::LoadedLocalStatusResponseSubject { status, .. } = subject;

    crate::with_d1_bookmark(
        Response::from_json(&StatusSourceResponse {
            id: status.id,
            text: status.text,
            spoiler_text: status.spoiler_text,
        })?,
        &detail.base.session,
    )
}

pub(crate) async fn status_context_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(detail) = resolve_status_detail_request_context(&req, &ctx).await? else {
        return Response::error("missing status id route parameter", 400);
    };
    let Some(status) =
        resolve_status_reference(&detail.base.db, &detail.base.config, &detail.base.status_id)
            .await?
    else {
        return Response::error("status not found", 404);
    };

    let response = match status {
        ResolvedStatus::Local(status) => {
            let Some(subject) = load_visible_local_status_response_subject(
                &detail.base.db,
                detail.viewer.as_ref(),
                status,
            )
            .await?
            else {
                return Response::error("status not found", 404);
            };
            let super::LoadedLocalStatusResponseSubject {
                status,
                account: owner,
                ..
            } = subject;

            let context = build_local_status_context(
                &detail.base.db,
                &detail.base.config,
                detail.viewer.as_ref(),
                &status,
                &owner,
            )
            .await?;
            context_response_with_async_refresh(
                &detail.base.db,
                &status.id,
                detail.viewer.is_some(),
                &context,
            )
            .await?
        }
        ResolvedStatus::Remote(status) => {
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
                return Response::error("status not found", 404);
            }
            let Some(actor) =
                find_remote_actor_by_actor_uri(&detail.base.db, &status.actor_uri).await?
            else {
                return Response::error("status not found", 404);
            };
            let context = build_remote_status_context(
                &detail.base.db,
                &detail.base.config,
                detail.viewer.as_ref(),
                &status,
                &actor,
            )
            .await?;
            context_response_with_async_refresh(
                &detail.base.db,
                &status.id,
                detail.viewer.is_some(),
                &context,
            )
            .await?
        }
    };
    crate::with_d1_bookmark(response, &detail.base.session)
}

pub(crate) async fn status_history_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(detail) = resolve_status_detail_request_context(&req, &ctx).await? else {
        return Response::error("missing status id route parameter", 400);
    };

    if let Some(subject) = find_visible_local_status_response_subject(
        &detail.base.db,
        detail.viewer.as_ref(),
        &detail.base.status_id,
    )
    .await?
    {
        let super::LoadedLocalStatusResponseSubject {
            status,
            account,
            preload,
        } = subject;
        let response = build_local_status_response(
            &detail.base.db,
            &detail.base.config,
            detail.viewer.as_ref(),
            &status,
            &account,
            preload.in_reply_to_account_id,
            preload.media,
        )
        .await?;
        let created_at = crate::load_status_updated_at(&detail.base.db, &status.id)
            .await?
            .unwrap_or_else(|| status.created_at.clone());
        return crate::with_d1_bookmark(
            status_history_response_from_parts(
                response,
                created_at,
                crate::list_status_edit_snapshots(&detail.base.db, &status.id).await?,
            )?,
            &detail.base.session,
        );
    }

    if let Some(status) = find_remote_status_by_id(&detail.base.db, &detail.base.status_id).await? {
        if !is_public_activitypub_visibility(status.visibility.as_str()) {
            return Response::error("status not found", 404);
        }
        let Some(actor) =
            find_remote_actor_by_actor_uri(&detail.base.db, &status.actor_uri).await?
        else {
            return Response::error("status not found", 404);
        };

        let response = build_remote_status_response(
            &detail.base.db,
            &detail.base.config,
            detail.viewer.as_ref(),
            &status,
            &actor,
        )
        .await?;
        let created_at = load_remote_status_updated_at(&detail.base.db, &status.id)
            .await?
            .unwrap_or_else(|| status.published_at.clone());
        return crate::with_d1_bookmark(
            status_history_response_from_parts(
                response,
                created_at,
                list_remote_status_edit_snapshots(&detail.base.db, &status.id).await?,
            )?,
            &detail.base.session,
        );
    }

    Response::error("status not found", 404)
}

#[cfg(test)]
mod tests {
    use super::status_oembed_discovery_link;
    use crate::AppConfig;

    #[test]
    fn status_oembed_discovery_link_is_present_and_url_encoded() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let status_url = "https://social.example/@alice/statuses/status 1";
        let link = status_oembed_discovery_link(&config, status_url);
        assert!(link.contains("rel=\"alternate\""));
        assert!(link.contains("type=\"application/json+oembed\""));
        assert!(link.contains("/api/oembed?url="));
        assert!(link.contains(&*urlencoding::encode(status_url)));
        assert!(!link.contains("status 1"));
    }
}
