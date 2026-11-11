//! Shared plumbing for Mastodon's `/api/v1/admin/*` resources.

use crate::request_utils::{build_internal_cursor_link_header, parse_optional_bool};
use serde::Deserialize;
use serde_json::{Map, Value};
use worker::{Request, Response, Result, RouteContext};

#[derive(Debug, Default, Deserialize)]
pub(super) struct AdminPageQuery {
    pub(super) limit: Option<u32>,
    pub(super) max_id: Option<String>,
    pub(super) since_id: Option<String>,
    pub(super) min_id: Option<String>,
}

/// A page of admin records with Mastodon's `Link` header over their ids.
pub(super) fn admin_page_response(
    req: &Request,
    limit: u32,
    ids: impl Iterator<Item = i64>,
    documents: Vec<Value>,
) -> Result<Response> {
    let ids: Vec<i64> = ids.collect();
    let mut builder = Response::builder();
    if let Some(link) = build_internal_cursor_link_header(
        req,
        limit,
        ids.first().copied(),
        ids.last().copied(),
        ids.len() >= limit as usize,
        !ids.is_empty(),
    )? {
        builder = builder.with_header("Link", &link)?;
    }
    builder.from_json(&documents)
}

/// The integer `:id` of a record route; any other value names no record.
pub(super) fn admin_route_id(ctx: &RouteContext<()>) -> Option<i64> {
    ctx.param("id")?.trim().parse().ok()
}

pub(super) fn admin_record_not_found() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({ "error": "Record not found" }))?.with_status(404))
}

pub(super) fn admin_validation_error(message: &str) -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({ "error": message }))?.with_status(422))
}

/// A Mastodon admin request body, read from JSON or a form into one shape.
#[derive(Debug, Default)]
pub(super) struct AdminParams(Map<String, Value>);

impl AdminParams {
    /// `keys` names the form fields to read; JSON bodies keep every field.
    pub(super) async fn from_request(
        req: &mut Request,
        keys: &[&str],
    ) -> std::result::Result<Self, String> {
        let content_type = req
            .headers()
            .get("Content-Type")
            .map_err(|error| format!("failed to read Content-Type header: {error}"))?
            .unwrap_or_default()
            .to_ascii_lowercase();
        if content_type.contains("application/json") {
            return match req.json::<Value>().await {
                Ok(Value::Object(map)) => Ok(Self(map)),
                Ok(_) => Err("request body must be a JSON object".to_owned()),
                Err(error) => Err(format!("invalid JSON payload: {error}")),
            };
        }
        if content_type.trim().is_empty() {
            return Ok(Self::default());
        }
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form payload: {error}"))?;
        Ok(Self(
            keys.iter()
                .filter_map(|key| {
                    form.get_field(key)
                        .map(|value| ((*key).to_owned(), Value::String(value)))
                })
                .collect(),
        ))
    }

    #[cfg(test)]
    pub(super) fn from_json(value: Value) -> Self {
        match value {
            Value::Object(map) => Self(map),
            _ => Self::default(),
        }
    }

    /// `None` when absent; `Some(None)` when null or blank.
    pub(super) fn text(&self, key: &str) -> Option<Option<String>> {
        let value = self.0.get(key)?;
        Some(match value {
            Value::Null => None,
            Value::String(text) => Some(text.trim().to_owned()).filter(|text| !text.is_empty()),
            other => Some(other.to_string()),
        })
    }

    /// `Ok(None)` when absent, null, or blank.
    pub(super) fn bool(&self, key: &str) -> std::result::Result<Option<bool>, String> {
        match self.0.get(key) {
            Some(Value::Bool(value)) => Ok(Some(*value)),
            Some(Value::Number(number)) => Ok(Some(number.as_f64() != Some(0.0))),
            _ => parse_optional_bool(self.text(key).flatten().as_deref())
                .map_err(|_| format!("Validation failed: {key} is invalid")),
        }
    }
}
