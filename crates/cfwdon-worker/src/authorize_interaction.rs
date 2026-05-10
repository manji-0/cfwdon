use super::{
    AccountReference, FollowAccountRequest, Request, Response, Result, RouteContext,
    build_relationship_for_target, escape_html, follow_remote_account, load_config,
    require_authenticated_local_account, resolve_account_reference, resolve_search_account,
    upsert_local_follow,
};
use worker::ResponseBody;

#[derive(Debug, serde::Deserialize)]
struct AuthorizeInteractionQuery {
    uri: String,
}

pub(crate) async fn authorize_interaction_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AuthorizeInteractionQuery = req.query()?;
    let db = ctx.d1(&config.database_binding)?;
    let account = match resolve_search_account(&db, &config, &query.uri).await? {
        Some(account) => account,
        None => return Response::error("remote interaction target not found", 404),
    };

    html_response(&authorize_interaction_document(
        &query.uri,
        &account.display_name,
        &account.acct,
        &account.url,
    ))
}

pub(crate) async fn authorize_interaction_submit_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let uri = authorize_interaction_uri(&mut req).await?;
    let db = ctx.d1(&config.database_binding)?;
    let follower = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let account = match resolve_search_account(&db, &config, &uri).await? {
        Some(account) => account,
        None => return Response::error("remote interaction target not found", 404),
    };

    let relationship = match resolve_account_reference(&db, &account.id).await? {
        Some(AccountReference::Local(target)) => {
            if follower.id == target.id {
                return Response::error("cannot follow your own account", 422);
            }
            upsert_local_follow(
                &db,
                &config,
                &follower,
                &target,
                &FollowAccountRequest::default(),
            )
            .await?;
            build_relationship_for_target(&db, &config, &follower, &target.id, &account.uri).await?
        }
        Some(AccountReference::Remote(actor)) => {
            follow_remote_account(
                &db,
                &config,
                &follower,
                &actor,
                &FollowAccountRequest::default(),
            )
            .await?
        }
        None => return Response::error("remote interaction target not found", 404),
    };

    html_response(&authorize_interaction_success_document(
        &account.display_name,
        &account.acct,
        &account.url,
        relationship.requested,
    ))
}

async fn authorize_interaction_uri(req: &mut Request) -> Result<String> {
    let content_type = req
        .headers()
        .get("Content-Type")?
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.contains("application/x-www-form-urlencoded")
        || content_type.contains("multipart/form-data")
    {
        let form = req.form_data().await.map_err(|error| {
            worker::Error::RustError(format!("invalid authorize interaction form: {error}"))
        })?;
        return form
            .get_field("uri")
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| worker::Error::RustError("uri is required".to_owned()));
    }

    let query: AuthorizeInteractionQuery = req.query()?;
    Ok(query.uri)
}

pub(crate) fn authorize_interaction_document(
    uri: &str,
    display_name: &str,
    acct: &str,
    url: &str,
) -> String {
    authorize_interaction_page(
        "Remote follow",
        &format!(
            r#"<p class="lead">Follow <a href="{url}">{display_name}</a> from this server.</p><p class="acct">{acct}</p><form method="post" action="/authorize_interaction"><input type="hidden" name="uri" value="{uri}"><button type="submit">Follow</button></form>"#,
            url = escape_html(url),
            display_name = escape_html(display_name),
            acct = escape_html(acct),
            uri = escape_html(uri),
        ),
    )
}

fn authorize_interaction_success_document(
    display_name: &str,
    acct: &str,
    url: &str,
    requested: bool,
) -> String {
    let message = if requested {
        "Follow request sent"
    } else {
        "Now following"
    };
    authorize_interaction_page(
        message,
        &format!(
            r#"<p class="lead">{message} <a href="{url}">{display_name}</a>.</p><p class="acct">{acct}</p><p><a class="button" href="{url}">View profile</a></p>"#,
            url = escape_html(url),
            display_name = escape_html(display_name),
            acct = escape_html(acct),
        ),
    )
}

fn authorize_interaction_page(title: &str, body: &str) -> String {
    let title = escape_html(title);
    format!(
        r#"<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>
:root{{color-scheme:dark;--bg:#101114;--panel:#181b20;--line:#30343c;--text:#f4f0e8;--muted:#a9adb7;--accent:#45c08d;--ink:#0f1411}}
*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;display:grid;place-items:center;background:#101114;color:var(--text);font-family:Inter,ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;line-height:1.5}}main{{width:min(560px,100%);padding:24px}}section{{border:1px solid var(--line);border-radius:8px;background:var(--panel);padding:28px}}h1{{margin:0 0 12px;font-size:32px;line-height:1.1;letter-spacing:0}}a{{color:inherit}}.lead{{margin:0 0 10px;font-size:18px}}.acct{{margin:0 0 22px;color:var(--muted);overflow-wrap:anywhere}}button,.button{{display:inline-flex;align-items:center;justify-content:center;min-height:42px;padding:0 16px;border-radius:8px;border:1px solid var(--accent);background:var(--accent);color:var(--ink);font:inherit;font-weight:650;text-decoration:none;cursor:pointer}}
</style>
</head>
<body><main><section><h1>{title}</h1>{body}</section></main></body>
</html>"#
    )
}

fn html_response(html: &str) -> Result<Response> {
    let mut response = Response::from_body(ResponseBody::Body(html.as_bytes().to_vec()))?;
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    response.headers_mut().set("Cache-Control", "no-store")?;
    Ok(response)
}
