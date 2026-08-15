use crate::{
    ADMIN_UI_INDEX_PATH, auth0_login_redirect_response, auth0_logout_redirect_response,
    auth0_relogin_redirect_response, escape_html, find_authenticated_local_account_with_roles,
    instance_base_url, is_admin_authorized, load_config, serve_ui_asset,
};
use url::Url;
use worker::{Request, Response, ResponseBody, Result, RouteContext};

pub(crate) async fn admin_ui_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let path = req.path();
    if is_public_admin_asset_path(&path) {
        return serve_ui_asset(&ctx.env, &path).await;
    }
    if path == "/admin/logout" {
        return auth0_logout_redirect_response(&config);
    }
    if path == "/admin/relogin" {
        return admin_relogin_redirect(&config, &req);
    }

    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match find_authenticated_local_account_with_roles(&req, &db, &config).await? {
        Some((account, roles)) if is_admin_authorized(&config, &account, &roles) => account,
        Some(_) => return forbidden_html_response(),
        None => return admin_login_redirect(&config, &req),
    };

    let _ = account;
    serve_ui_asset(&ctx.env, ADMIN_UI_INDEX_PATH).await
}

pub(crate) fn is_admin_ui_path(path: &str) -> bool {
    path == "/admin" || path.starts_with("/admin/")
}

fn is_public_admin_asset_path(path: &str) -> bool {
    path.starts_with("/admin/assets/")
}

fn admin_login_redirect(config: &crate::AppConfig, req: &Request) -> Result<Response> {
    let return_url = admin_return_url(config, req)?;
    auth0_login_redirect_response(config, &return_url, &return_url)
}

fn admin_relogin_redirect(config: &crate::AppConfig, req: &Request) -> Result<Response> {
    let return_url = admin_return_url(config, req)?;
    auth0_relogin_redirect_response(config, &return_url)
}

fn admin_return_url(config: &crate::AppConfig, req: &Request) -> Result<Url> {
    let mut return_url = req.url()?;
    return_url.set_path("/admin/");
    return_url.set_query(None);
    if return_url.host_str().is_none() {
        return_url =
            Url::parse(&format!("{}/admin/", instance_base_url(config))).map_err(|error| {
                worker::Error::RustError(format!("invalid admin return URL: {error}"))
            })?;
    }
    Ok(return_url)
}

fn forbidden_html_response() -> Result<Response> {
    let body = forbidden_html_document();
    let mut response =
        Response::from_body(ResponseBody::Body(body.as_bytes().to_vec()))?.with_status(403);
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    response.headers_mut().set("Cache-Control", "no-store")?;
    Ok(response)
}

fn forbidden_html_document() -> String {
    let relogin_url = escape_html("/admin/relogin");
    let logout_url = escape_html("/admin/logout");
    format!(
        r#"<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Forbidden</title>
<style>
:root{{color-scheme:dark;--bg:#101114;--panel:#181b20;--line:#30343c;--text:#f4f0e8;--muted:#a9adb7;--accent:#45c08d;--ink:#0f1411}}
*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;display:grid;place-items:center;background:var(--bg);color:var(--text);font-family:Inter,ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;line-height:1.5}}main{{width:min(560px,100%);padding:24px}}section{{border:1px solid var(--line);border-radius:8px;background:var(--panel);padding:28px}}h1{{margin:0 0 12px;font-size:32px;line-height:1.1}}p{{margin:0 0 20px;color:var(--muted)}}.actions{{display:flex;flex-wrap:wrap;gap:12px}}a.button{{display:inline-flex;align-items:center;justify-content:center;min-height:42px;padding:0 16px;border-radius:8px;border:1px solid var(--accent);background:var(--accent);color:var(--ink);font:inherit;font-weight:650;text-decoration:none}}a.button.secondary{{background:transparent;color:var(--text);border-color:var(--line)}}
</style>
</head>
<body>
<main>
<section>
<h1>403 Forbidden</h1>
<p>管理者権限が必要です。別のアカウントで再ログインするか、ログアウトしてください。</p>
<div class="actions">
<a class="button" href="{relogin_url}">再ログイン</a>
<a class="button secondary" href="{logout_url}">ログアウト</a>
</div>
</section>
</main>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::{forbidden_html_document, is_admin_ui_path, is_public_admin_asset_path};

    #[test]
    fn admin_ui_paths_are_detected() {
        assert!(is_admin_ui_path("/admin"));
        assert!(is_admin_ui_path("/admin/"));
        assert!(is_admin_ui_path("/admin/reports"));
        assert!(is_admin_ui_path("/admin/relogin"));
        assert!(is_admin_ui_path("/admin/logout"));
        assert!(!is_admin_ui_path("/api/cfwdon/admin/me"));
    }

    #[test]
    fn public_asset_paths_skip_auth_gate() {
        assert!(is_public_admin_asset_path("/admin/assets/index-abc.js"));
        assert!(!is_public_admin_asset_path("/admin/"));
    }

    #[test]
    fn forbidden_page_includes_relogin_and_logout_actions() {
        let html = forbidden_html_document();
        assert!(html.contains(r#"href="/admin/relogin""#));
        assert!(html.contains(r#"href="/admin/logout""#));
        assert!(html.contains("再ログイン"));
        assert!(html.contains("ログアウト"));
    }
}
