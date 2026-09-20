mod cache;
mod document;
mod interaction_policy;
mod languages;
mod provider;
mod provider_client;
mod provider_languages;

// The cache fingerprint and provider request/response helpers are exercised
// from `unit_tests`, so the lib build sees these re-exports as unused.
#[allow(unused_imports)]
pub(crate) use cache::*;
pub(crate) use document::*;
pub(crate) use interaction_policy::*;
pub(crate) use languages::*;
pub(crate) use provider::*;
#[allow(unused_imports)]
pub(crate) use provider_client::*;
pub(crate) use provider_languages::*;

use super::{
    Request, Response, Result, RouteContext, app_bearer_token_from_request,
    configured_instance_languages, find_authenticated_local_account,
    find_oauth_app_by_bearer_token, load_config, oauth_app_has_any_scope, status_api_response,
};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
struct TranslateStatusRequest {
    lang: Option<String>,
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

fn translation_status_visibility_allows_translation(visibility: Option<&str>) -> bool {
    !matches!(visibility.unwrap_or("public"), "private" | "direct")
}

fn translation_language_pair_allows_translation(
    source_language: &str,
    target_language: &str,
) -> bool {
    target_language != source_language
}

pub(crate) async fn translate_status_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let bearer_token = app_bearer_token_from_request(&req)?;
    let app = match bearer_token.as_deref() {
        Some(token) => match find_oauth_app_by_bearer_token(&db, token).await? {
            Some(app) => {
                if !oauth_app_has_any_scope(&app, &["read:statuses", "read"]) {
                    return Ok(Response::from_json(&serde_json::json!({
                        "error": "This action is outside the authorized scopes",
                    }))?
                    .with_status(403));
                }
                Some(app)
            }
            None => {
                return Ok(Response::from_json(&serde_json::json!({
                    "error": "The access token is invalid",
                }))?
                .with_status(401));
            }
        },
        None => None,
    };
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    if viewer.is_none() && app.is_none() {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "The access token is invalid",
        }))?
        .with_status(401));
    };
    let request = match parse_translate_status_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    let route_status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let provider_config = configured_translation_provider(&ctx);

    let mut response = status_api_response(req, ctx).await?;
    if response.status_code() != 200 {
        return Response::error("Record not found", 404);
    }
    let value = response.json::<serde_json::Value>().await?;
    let visibility = value.get("visibility").and_then(serde_json::Value::as_str);
    if !translation_status_visibility_allows_translation(visibility) {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "This action is not allowed",
        }))?
        .with_status(403));
    }

    let source_language = value
        .get("language")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("und");
    let viewer_default_language = viewer
        .as_ref()
        .and_then(|viewer| viewer.default_language().map(str::to_owned));
    let target_language = translation_target_language(
        request.lang.as_deref(),
        viewer_default_language.as_deref(),
        &configured_instance_languages(&config),
        source_language,
    );
    if !translation_language_pair_allows_translation(source_language, &target_language) {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "This action is not allowed",
        }))?
        .with_status(403));
    }

    if let Some(provider_config) = provider_config {
        let Some(document) = cached_or_fresh_provider_translation_document(
            &db,
            &value,
            source_language,
            &target_language,
            &route_status_id,
            &provider_config,
        )
        .await?
        else {
            return Ok(Response::from_json(&serde_json::json!({
                "error": "This action is not allowed",
            }))?
            .with_status(403));
        };
        return Response::from_json(&document);
    }

    Ok(Response::from_json(&serde_json::json!({
        "error": "Translation provider is not configured",
    }))?
    .with_status(503))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_status_visibility_blocks_private_and_direct_statuses() {
        assert!(translation_status_visibility_allows_translation(None));
        assert!(translation_status_visibility_allows_translation(Some(
            "public"
        )));
        assert!(translation_status_visibility_allows_translation(Some(
            "unlisted"
        )));
        assert!(!translation_status_visibility_allows_translation(Some(
            "private"
        )));
        assert!(!translation_status_visibility_allows_translation(Some(
            "direct"
        )));
    }

    #[test]
    fn translation_language_pair_blocks_noop_translations() {
        assert!(translation_language_pair_allows_translation("en", "ja"));
        assert!(!translation_language_pair_allows_translation("en", "en"));
    }
}
