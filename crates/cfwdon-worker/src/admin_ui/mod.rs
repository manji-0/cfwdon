mod assets {
    include!(concat!(env!("OUT_DIR"), "/admin_ui_assets.rs"));
}

use crate::{
    auth0_login_redirect_response, find_authenticated_local_account, is_admin_account, load_config,
};
use assets::lookup_embedded_asset;
use worker::{Request, Response, ResponseBody, Result, RouteContext};

pub(crate) async fn admin_ui_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let path = req.path();
    if is_public_admin_asset_path(&path) {
        return serve_embedded_asset(&path);
    }

    let db = ctx.d1(&config.database_binding)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) if is_admin_account(&config, &account) => account,
        Some(_) => return forbidden_html_response(),
        None => return admin_login_redirect(&config, &req),
    };

    let _ = account;
    serve_embedded_asset("/admin/")
}

pub(crate) fn is_admin_ui_path(path: &str) -> bool {
    path == "/admin" || path.starts_with("/admin/")
}

fn is_public_admin_asset_path(path: &str) -> bool {
    path.starts_with("/admin/assets/")
}

fn serve_embedded_asset(path: &str) -> Result<Response> {
    let (bytes, content_type) = lookup_embedded_asset(path)
        .ok_or_else(|| worker::Error::RustError(format!("admin ui asset not found: {path}")))?;
    let mut response = Response::from_bytes(bytes.to_vec())?;
    response.headers_mut().set("Content-Type", content_type)?;
    response
        .headers_mut()
        .set("Cache-Control", "public, max-age=3600")?;
    Ok(response)
}

fn admin_login_redirect(config: &crate::AppConfig, req: &Request) -> Result<Response> {
    let return_url = req.url()?;
    auth0_login_redirect_response(config, &return_url, &return_url)
}

fn forbidden_html_response() -> Result<Response> {
    let body = "<!doctype html><html lang=\"ja\"><head><meta charset=\"utf-8\"><title>Forbidden</title></head><body><main><h1>403 Forbidden</h1><p>管理者権限が必要です。</p></main></body></html>";
    let mut response =
        Response::from_body(ResponseBody::Body(body.as_bytes().to_vec()))?.with_status(403);
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    response.headers_mut().set("Cache-Control", "no-store")?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::{is_admin_ui_path, is_public_admin_asset_path};

    #[test]
    fn admin_ui_paths_are_detected() {
        assert!(is_admin_ui_path("/admin"));
        assert!(is_admin_ui_path("/admin/"));
        assert!(is_admin_ui_path("/admin/reports"));
        assert!(!is_admin_ui_path("/api/cfwdon/admin/me"));
    }

    #[test]
    fn public_asset_paths_skip_auth_gate() {
        assert!(is_public_admin_asset_path("/admin/assets/index-abc.js"));
        assert!(!is_public_admin_asset_path("/admin/"));
    }
}
