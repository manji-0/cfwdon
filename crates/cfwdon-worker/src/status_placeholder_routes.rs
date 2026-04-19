use crate::{
    Request, Response, Result, RouteContext, find_authenticated_local_account, load_config,
    status_api_response,
};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
struct InteractionPolicyUpdateRequest {
    quote_approval_policy: Option<String>,
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

pub(crate) fn build_translation_document(status: &serde_json::Value) -> serde_json::Value {
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
        "language": source_language,
        "poll": poll,
        "media_attachments": media_attachments,
        "detected_source_language": source_language,
        "provider": "cfwdon-placeholder",
    })
}

pub(crate) async fn status_interaction_policy_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    if find_authenticated_local_account(&req, &db, &config)
        .await?
        .is_none()
    {
        return Response::error("Cloudflare Access authentication required", 401);
    }
    if let Err(message) = parse_interaction_policy_update_request(&mut req).await {
        return Response::error(message, 422);
    }
    status_api_response(req, ctx).await
}

pub(crate) async fn translate_status_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    if find_authenticated_local_account(&req, &db, &config)
        .await?
        .is_none()
    {
        return Response::error("Cloudflare Access authentication required", 401);
    }

    let mut response = status_api_response(req, ctx).await?;
    let value = response.json::<serde_json::Value>().await?;
    Response::from_json(&build_translation_document(&value))
}
