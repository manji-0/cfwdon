use crate::{
    Request, Response, Result, RouteContext, load_config, require_authenticated_local_account,
};

pub(crate) async fn suggestions_v1_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    if require_authenticated_local_account(&req, &db, &config)
        .await?
        .is_none()
    {
        return Response::error("Cloudflare Access authentication required", 401);
    }

    Response::from_json(&Vec::<serde_json::Value>::new())
}

pub(crate) async fn delete_suggestion_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    if require_authenticated_local_account(&req, &db, &config)
        .await?
        .is_none()
    {
        return Response::error("Cloudflare Access authentication required", 401);
    }

    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn suggestions_v2_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    if require_authenticated_local_account(&req, &db, &config)
        .await?
        .is_none()
    {
        return Response::error("Cloudflare Access authentication required", 401);
    }

    Response::from_json(&Vec::<serde_json::Value>::new())
}
