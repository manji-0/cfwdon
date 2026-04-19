use crate::{Request, Response, Result, RouteContext, extract_authenticated_user, load_config};

pub(crate) fn scheduled_status_document(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "scheduled_at": "2099-01-01T00:00:00.000Z",
        "params": {
            "poll": serde_json::Value::Null,
            "text": "",
            "language": serde_json::Value::Null,
            "media_ids": serde_json::Value::Null,
            "sensitive": serde_json::Value::Null,
            "visibility": serde_json::Value::Null,
            "idempotency": serde_json::Value::Null,
            "scheduled_at": serde_json::Value::Null,
            "spoiler_text": serde_json::Value::Null,
            "application_id": 0,
            "in_reply_to_id": serde_json::Value::Null,
            "with_rate_limit": false,
        },
        "media_attachments": [],
    })
}

pub(crate) async fn scheduled_statuses_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    match extract_authenticated_user(&req, &config).await? {
        Some(_) => Response::from_json(&Vec::<serde_json::Value>::new()),
        None => Response::error("Cloudflare Access authentication required", 401),
    }
}

pub(crate) async fn scheduled_status_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    match extract_authenticated_user(&req, &config).await? {
        Some(_) => {
            let id = ctx
                .param("id")
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    worker::Error::RustError("missing scheduled status id".to_owned())
                })?;
            Response::from_json(&scheduled_status_document(&id))
        }
        None => Response::error("Cloudflare Access authentication required", 401),
    }
}

pub(crate) async fn update_scheduled_status_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    scheduled_status_response(req, ctx).await
}

pub(crate) async fn delete_scheduled_status_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    match extract_authenticated_user(&req, &config).await? {
        Some(_) => Response::from_json(&serde_json::json!({})),
        None => Response::error("Cloudflare Access authentication required", 401),
    }
}
