use crate::{
    Request, Response, Result, RouteContext, build_local_status_response, find_account_by_id,
    find_authenticated_local_account, find_media_attachments_by_status_id, find_status_by_id,
    load_config, load_in_reply_to_account_id, now_iso_string, status_api_response,
    update_local_status_quote_approval_policy,
};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
struct InteractionPolicyUpdateRequest {
    quote_approval_policy: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct TranslateStatusRequest {
    lang: Option<String>,
}

pub(crate) fn normalize_quote_approval_policy(
    value: Option<String>,
) -> std::result::Result<Option<String>, String> {
    let value = value
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    match value.as_deref() {
        None | Some("public") | Some("followers") | Some("nobody") => Ok(value),
        Some(_) => {
            Err("quote_approval_policy must be one of: public, followers, nobody".to_owned())
        }
    }
}

async fn parse_interaction_policy_update_request(
    req: &mut Request,
) -> std::result::Result<Option<String>, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let policy = if content_type.contains("application/json") {
        req.json::<InteractionPolicyUpdateRequest>()
            .await
            .map_err(|error| format!("invalid JSON interaction policy payload: {error}"))?
            .quote_approval_policy
    } else {
        req.form_data()
            .await
            .map_err(|error| format!("invalid form interaction policy payload: {error}"))?
            .get_field("quote_approval_policy")
    };

    normalize_quote_approval_policy(policy)
}

pub(crate) fn build_translation_document_for_language(
    status: &serde_json::Value,
    target_language: &str,
    provider: &str,
) -> serde_json::Value {
    let source_language = status
        .get("language")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("und");
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
    let poll = status
        .get("poll")
        .and_then(serde_json::Value::as_object)
        .map(|poll| {
            serde_json::json!({
                "id": poll.get("id").cloned().unwrap_or_else(|| serde_json::json!("")),
                "options": poll
                    .get("options")
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
                    .unwrap_or_default(),
            })
        })
        .unwrap_or(serde_json::Value::Null);

    serde_json::json!({
        "content": status.get("content").cloned().unwrap_or_else(|| serde_json::json!("")),
        "spoiler_text": status.get("spoiler_text").cloned().unwrap_or_else(|| serde_json::json!("")),
        "language": target_language,
        "poll": poll,
        "media_attachments": media_attachments,
        "detected_source_language": source_language,
        "provider": provider,
    })
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_translation_document(status: &serde_json::Value) -> serde_json::Value {
    let source_language = status
        .get("language")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("und");
    build_translation_document_for_language(status, source_language, "cfwdon-placeholder")
}

async fn parse_translate_status_request(
    req: &mut Request,
) -> std::result::Result<TranslateStatusRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut request = if content_type.contains("application/json") {
        req.json::<TranslateStatusRequest>()
            .await
            .map_err(|error| format!("invalid JSON translation payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form translation payload: {error}"))?;
        TranslateStatusRequest {
            lang: form.get_field("lang"),
        }
    };

    if let Some(lang) = request.lang.as_mut() {
        *lang = lang.trim().to_ascii_lowercase();
        if lang.is_empty() {
            request.lang = None;
        }
    }

    Ok(request)
}

pub(crate) async fn status_interaction_policy_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let Some(viewer) = find_authenticated_local_account(&req, &db, &config).await? else {
        return Response::error("Cloudflare Access authentication required", 401);
    };
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing status id route parameter".to_owned()))?;
    let requested_policy = match parse_interaction_policy_update_request(&mut req).await {
        Ok(policy) => policy,
        Err(message) => return Response::error(message, 422),
    };
    let Some(status) = find_status_by_id(&db, &status_id).await? else {
        return Response::error("status not found", 404);
    };
    if status.account_id != viewer.id {
        return Response::error("status not found", 404);
    }
    let effective_policy = match requested_policy.as_deref() {
        Some(_) if matches!(status.visibility.as_str(), "private" | "direct") => "nobody",
        Some(policy) => policy,
        None => crate::effective_local_quote_approval_policy(&status),
    };
    let updated_at = now_iso_string()?;
    let updated =
        update_local_status_quote_approval_policy(&db, &status, effective_policy, &updated_at)
            .await?;
    let Some(account) = find_account_by_id(&db, &updated.account_id).await? else {
        return Response::error("status not found", 404);
    };
    let media = find_media_attachments_by_status_id(&db, &updated.id).await?;
    let in_reply_to_account_id = load_in_reply_to_account_id(&db, &updated).await?;
    Response::from_json(
        &build_local_status_response(
            &db,
            &config,
            Some(&viewer),
            &updated,
            &account,
            in_reply_to_account_id,
            media,
        )
        .await?,
    )
}

pub(crate) async fn translate_status_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let Some(viewer) = find_authenticated_local_account(&req, &db, &config).await? else {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "The access token is invalid",
        }))?
        .with_status(401));
    };
    let request = match parse_translate_status_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };

    let mut response = status_api_response(req, ctx).await?;
    if response.status_code() != 200 {
        return Response::error("Record not found", 404);
    }
    let value = response.json::<serde_json::Value>().await?;
    let visibility = value
        .get("visibility")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("public");
    if matches!(visibility, "private" | "direct") {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "This action is not allowed",
        }))?
        .with_status(403));
    }

    let source_language = value
        .get("language")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("und");
    let target_language = request
        .lang
        .or_else(|| viewer.default_language.clone())
        .unwrap_or_else(|| source_language.to_owned());
    if target_language == source_language {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "This action is not allowed",
        }))?
        .with_status(403));
    }

    Response::from_json(&build_translation_document_for_language(
        &value,
        &target_language,
        "cfwdon-placeholder",
    ))
}
