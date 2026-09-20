use crate::statuses::{
    Request, Response, Result, RouteContext, app_bearer_token_from_request,
    build_loaded_local_status_response, find_account_by_id, find_authenticated_local_account,
    find_status_by_id, load_config, now_iso_string, update_local_status_quote_approval_policy,
};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub(super) struct InteractionPolicyUpdateRequest {
    quote_approval_policy: Option<String>,
}

pub(crate) fn normalize_quote_approval_policy(
    value: Option<String>,
) -> std::result::Result<Option<cfwdon_domain::QuoteApprovalPolicy>, String> {
    let value = value
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    match value.as_deref() {
        None => Ok(None),
        Some(value) => cfwdon_domain::QuoteApprovalPolicy::parse(value)
            .map(Some)
            .map_err(|error| error.to_string()),
    }
}

pub(super) async fn parse_interaction_policy_update_request(
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
        .map(|policy| policy.map(|policy| policy.as_str().to_owned()))
}

pub(crate) async fn status_interaction_policy_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    if app_bearer_token_from_request(&req)?.is_some() {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "The access token is invalid",
        }))?
        .with_status(401));
    }
    let db = crate::bind_request_d1(&ctx, &config)?;
    let Some(viewer) = find_authenticated_local_account(&req, &db, &config).await? else {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "The access token is invalid",
        }))?
        .with_status(401));
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
    if status.account_id != viewer.id() {
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
    Response::from_json(
        &build_loaded_local_status_response(&db, &config, Some(&viewer), &updated, &account)
            .await?,
    )
}
