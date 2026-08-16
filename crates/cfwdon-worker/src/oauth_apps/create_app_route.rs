use super::{OAuthAppRow, oauth_app_redirect_uris, oauth_app_scopes};
use crate::id_utils::generate_entity_id;
use crate::runtime_config::load_config;
use serde::Deserialize;
use url::Url;
use worker::{FormData, Request, Response, Result, RouteContext, d1::D1Type};

#[derive(Debug, Default, Deserialize)]
struct CreateAppRequest {
    client_name: Option<String>,
    scopes: Option<String>,
    website: Option<String>,
}

#[derive(Debug)]
struct ParsedCreateAppRequest {
    client_name: String,
    website: Option<String>,
    scopes: Vec<String>,
    redirect_uris: Vec<String>,
}

fn app_document(row: &OAuthAppRow, config: &cfwdon_core::AppConfig) -> serde_json::Value {
    let scopes = oauth_app_scopes(row);
    let redirect_uris = oauth_app_redirect_uris(row);
    serde_json::json!({
        "id": row.id.to_string(),
        "name": row.name,
        "website": row.website,
        "scopes": scopes,
        "redirect_uri": row.redirect_uri_legacy,
        "redirect_uris": redirect_uris,
        "client_id": row.client_id,
        "client_secret": row.client_secret,
        "client_secret_expires_at": row.client_secret_expires_at,
        "vapid_key": config.web_push_vapid_public_key.as_deref().unwrap_or(""),
    })
}

fn normalize_required_client_name(value: Option<String>) -> std::result::Result<String, String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Validation failed: Name can't be blank".to_owned())
}

fn normalize_scopes(value: Option<String>) -> Vec<String> {
    let raw = value.unwrap_or_else(|| "read".to_owned());
    let mut scopes = Vec::new();
    for scope in raw.split_whitespace() {
        let normalized = scope.trim().to_owned();
        if !normalized.is_empty() && !scopes.contains(&normalized) {
            scopes.push(normalized);
        }
    }
    if scopes.is_empty() {
        vec!["read".to_owned()]
    } else {
        scopes
    }
}

fn normalize_website(value: Option<String>) -> std::result::Result<Option<String>, String> {
    let Some(value) = value.map(|value| value.trim().to_owned()) else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let url = Url::parse(&value)
        .map_err(|_| "Validation failed: Website must be a valid URL".to_owned())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Validation failed: Website must be a valid URL".to_owned());
    }
    Ok(Some(value))
}

fn validate_redirect_uri(value: &str) -> std::result::Result<String, String> {
    if value == "urn:ietf:wg:oauth:2.0:oob" {
        return Ok(value.to_owned());
    }
    let url = Url::parse(value)
        .map_err(|_| "Validation failed: Redirect URI must be an absolute URI.".to_owned())?;
    if matches!(url.scheme(), "http" | "https") && url.host_str().is_none() {
        return Err("Validation failed: Redirect URI must be an absolute URI.".to_owned());
    }
    Ok(value.to_owned())
}

fn normalize_redirect_uris(values: Vec<String>) -> std::result::Result<Vec<String>, String> {
    let mut redirect_uris = Vec::new();
    for value in values {
        for candidate in value.split_whitespace() {
            let candidate = candidate.trim();
            if candidate.is_empty() {
                continue;
            }
            let normalized = validate_redirect_uri(candidate)?;
            if !redirect_uris.contains(&normalized) {
                redirect_uris.push(normalized);
            }
        }
    }
    if redirect_uris.is_empty() {
        return Err("Validation failed: Redirect URI must be an absolute URI.".to_owned());
    }
    Ok(redirect_uris)
}

async fn parse_create_app_request(
    req: &mut Request,
) -> std::result::Result<ParsedCreateAppRequest, String> {
    let content_type = request_content_type(req)?;
    let (request, redirect_uris) = if request_is_json(&content_type) {
        create_app_request_from_json_payload(
            req.json::<serde_json::Value>()
                .await
                .map_err(|error| format!("invalid JSON app payload: {error}"))?,
        )?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form app payload: {error}"))?;
        create_app_request_from_form(&form)
    };

    parsed_create_app_request(request, redirect_uris)
}

fn request_content_type(req: &Request) -> std::result::Result<String, String> {
    Ok(req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase())
}

fn request_is_json(content_type: &str) -> bool {
    content_type.contains("application/json")
}

fn create_app_request_from_json_payload(
    payload: serde_json::Value,
) -> std::result::Result<(CreateAppRequest, Vec<String>), String> {
    let redirect_uris = redirect_uris_from_json_payload(&payload);
    let request = serde_json::from_value::<CreateAppRequest>(payload)
        .map_err(|error| format!("invalid JSON app payload: {error}"))?;
    Ok((request, redirect_uris))
}

fn redirect_uris_from_json_payload(payload: &serde_json::Value) -> Vec<String> {
    match payload.get("redirect_uris") {
        Some(serde_json::Value::String(value)) => vec![value.clone()],
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .collect(),
        _ => payload
            .get("redirect_uri")
            .and_then(serde_json::Value::as_str)
            .map(|value| vec![value.to_owned()])
            .unwrap_or_default(),
    }
}

fn create_app_request_from_form(form: &FormData) -> (CreateAppRequest, Vec<String>) {
    (
        CreateAppRequest {
            client_name: form.get_field("client_name"),
            scopes: form.get_field("scopes"),
            website: form.get_field("website"),
        },
        redirect_uris_from_form(form),
    )
}

fn redirect_uris_from_form(form: &FormData) -> Vec<String> {
    form.get_all("redirect_uris[]")
        .map(|entries| {
            entries
                .into_iter()
                .filter_map(|entry| match entry {
                    worker::FormEntry::Field(value) => Some(value),
                    worker::FormEntry::File(_) => None,
                })
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| {
            form.get_field("redirect_uris")
                .or_else(|| form.get_field("redirect_uri"))
                .map(|value| vec![value])
                .unwrap_or_default()
        })
}

fn parsed_create_app_request(
    request: CreateAppRequest,
    redirect_uris: Vec<String>,
) -> std::result::Result<ParsedCreateAppRequest, String> {
    Ok(ParsedCreateAppRequest {
        client_name: normalize_required_client_name(request.client_name)?,
        website: normalize_website(request.website)?,
        scopes: normalize_scopes(request.scopes),
        redirect_uris: normalize_redirect_uris(redirect_uris)?,
    })
}

async fn insert_oauth_app(
    db: &crate::D1Database,
    request: &ParsedCreateAppRequest,
) -> Result<OAuthAppRow> {
    let scopes_json = serde_json::to_string(&request.scopes).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize app scopes: {error}"))
    })?;
    let redirect_uris_json = serde_json::to_string(&request.redirect_uris).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize app redirect URIs: {error}"))
    })?;
    let redirect_uri_legacy = request.redirect_uris.join("\n");
    let client_id = generate_entity_id(24)?;
    let client_secret = generate_entity_id(32)?;
    let bindings = [
        D1Type::Text(request.client_name.as_str()),
        request
            .website
            .as_deref()
            .map(D1Type::Text)
            .unwrap_or(D1Type::Null),
        D1Type::Text(scopes_json.as_str()),
        D1Type::Text(redirect_uris_json.as_str()),
        D1Type::Text(redirect_uri_legacy.as_str()),
        D1Type::Text(client_id.as_str()),
        D1Type::Text(client_secret.as_str()),
    ];
    db.prepare(
        "INSERT INTO oauth_apps (
            name,
            website,
            scopes_json,
            redirect_uris_json,
            redirect_uri_legacy,
            client_id,
            client_secret,
            client_secret_expires_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            0
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let client_id_binding = D1Type::Text(client_id.as_str());
    db.prepare(
        "SELECT id, name, website, scopes_json, redirect_uri_legacy, redirect_uris_json,
                client_id, client_secret, client_secret_expires_at
         FROM oauth_apps
         WHERE client_id = ?1
         LIMIT 1",
    )
    .bind_refs(&client_id_binding)?
    .first::<OAuthAppRow>(None)
    .await?
    .ok_or_else(|| worker::Error::RustError("created app could not be reloaded".to_owned()))
}

pub(crate) async fn create_app_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let request = match parse_create_app_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(&message, 422),
    };
    let app = insert_oauth_app(&db, &request).await?;
    Response::from_json(&app_document(&app, &config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_app_request_from_json_payload_extracts_redirect_variants() {
        let payload = serde_json::json!({
            "client_name": "Client",
            "scopes": "read write",
            "website": "https://example.com",
            "redirect_uris": [
                "https://client.example/callback",
                42,
                "app://callback"
            ],
            "redirect_uri": "https://fallback.example/callback"
        });

        let (request, redirect_uris) =
            create_app_request_from_json_payload(payload).expect("app request");

        assert_eq!(request.client_name.as_deref(), Some("Client"));
        assert_eq!(request.scopes.as_deref(), Some("read write"));
        assert_eq!(request.website.as_deref(), Some("https://example.com"));
        assert_eq!(
            redirect_uris,
            vec!["https://client.example/callback", "app://callback"]
        );
    }

    #[test]
    fn create_app_request_from_json_payload_falls_back_to_redirect_uri() {
        let payload = serde_json::json!({
            "client_name": "Client",
            "redirect_uri": "https://client.example/callback"
        });

        let (_, redirect_uris) =
            create_app_request_from_json_payload(payload).expect("app request");

        assert_eq!(redirect_uris, vec!["https://client.example/callback"]);
    }

    #[test]
    fn parsed_create_app_request_normalizes_fields() {
        let parsed = parsed_create_app_request(
            CreateAppRequest {
                client_name: Some("  Client  ".to_owned()),
                scopes: Some("read write read".to_owned()),
                website: Some(" https://example.com/app ".to_owned()),
            },
            vec![
                " https://client.example/callback app://callback ".to_owned(),
                "app://callback".to_owned(),
            ],
        )
        .expect("parsed app request");

        assert_eq!(parsed.client_name, "Client");
        assert_eq!(parsed.website.as_deref(), Some("https://example.com/app"));
        assert_eq!(parsed.scopes, vec!["read", "write"]);
        assert_eq!(
            parsed.redirect_uris,
            vec!["https://client.example/callback", "app://callback"]
        );
    }

    #[test]
    fn parsed_create_app_request_rejects_blank_client_name() {
        let error = parsed_create_app_request(
            CreateAppRequest {
                client_name: Some("  ".to_owned()),
                ..CreateAppRequest::default()
            },
            vec!["https://client.example/callback".to_owned()],
        )
        .expect_err("blank client name");

        assert_eq!(error, "Validation failed: Name can't be blank");
    }
}
