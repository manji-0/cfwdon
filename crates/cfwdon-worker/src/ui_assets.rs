use crate::ui_asset_cache_control;
use worker::{Env, Response, Result};

pub(crate) const ASSETS_BINDING: &str = "ASSETS";
pub(crate) const WEB_UI_INDEX_PATH: &str = "/app/index.html";
pub(crate) const ADMIN_UI_INDEX_PATH: &str = "/admin/index.html";

pub(crate) async fn serve_ui_asset(env: &Env, asset_path: &str) -> Result<Response> {
    let assets = env.assets(ASSETS_BINDING)?;
    let mut response = assets
        .fetch(format!("https://assets.local{asset_path}"), None)
        .await?;
    if response.status_code() == 404 {
        return Response::error("Not Found", 404);
    }
    response
        .headers_mut()
        .set("Cache-Control", ui_asset_cache_control(asset_path))?;
    Ok(response)
}
