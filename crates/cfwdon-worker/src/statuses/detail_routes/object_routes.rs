use crate::statuses::{
    CACHE_TTL_FEDERATION, Error, Request, Response, Result, RouteContext, build_activitypub_note,
    cache_public_json_response, cache_public_response_with_options, find_account_by_username,
    find_remote_status_by_id, find_status_by_id, is_public_activitypub_visibility, load_config,
    load_local_status_response_preload, strip_html_tags,
};

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

pub(super) fn status_object_html_response(
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

pub(super) fn status_oembed_discovery_link(config: &crate::AppConfig, status_url: &str) -> String {
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
