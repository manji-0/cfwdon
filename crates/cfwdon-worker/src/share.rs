use super::{
    CreatePublishedStatusInput, Request, Response, Result, RouteContext,
    auth0_login_redirect_response, create_published_status_and_response,
    enqueue_outbox_process_queue_best_effort, escape_html, find_authenticated_local_account,
    invalidate_account_dynamic_public_cache, load_config,
};
use cfwdon_domain::{ComposingStatus, Visibility};
use worker::ResponseBody;

#[derive(Debug, Default, serde::Deserialize)]
struct ShareQuery {
    text: Option<String>,
    url: Option<String>,
    title: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct ShareForm {
    status: String,
}

pub(crate) async fn share_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: ShareQuery = req.query().unwrap_or_default();
    let db = ctx.d1(&config.database_binding)?;
    let Some(_account) = find_authenticated_local_account(&req, &db, &config).await? else {
        return share_login_redirect(&config, &req);
    };

    html_response(&share_document(&share_initial_text(
        query.title.as_deref(),
        query.url.as_deref(),
        query.text.as_deref(),
    )))
}

pub(crate) async fn share_submit_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_authenticated_local_account(&req, &db, &config).await? else {
        return share_login_redirect(&config, &req);
    };

    let form: ShareForm = match read_share_form(&mut req).await {
        Ok(form) => form,
        Err(message) => return Response::error(message, 422),
    };
    let draft = match share_status_draft(&form.status) {
        Ok(draft) => draft,
        Err(message) => {
            return html_response(&share_document_with_error(&form.status, &message));
        }
    };

    let response = create_published_status_and_response(
        &db,
        &config,
        CreatePublishedStatusInput {
            account: &account,
            application_id: None,
            draft: &draft,
            pending_media: &[],
            in_reply_to_account_id: None,
            quote_of_uri: None,
        },
    )
    .await?;
    invalidate_account_dynamic_public_cache(&ctx, account.id(), account.username()).await;
    enqueue_outbox_process_queue_best_effort(&ctx.env, "share_create").await;

    let redirect_url = url::Url::parse(&response.url).map_err(|error| {
        worker::Error::RustError(format!("invalid share redirect url: {error}"))
    })?;
    Response::redirect(redirect_url)
}

pub(crate) fn share_initial_text(
    title: Option<&str>,
    url: Option<&str>,
    text: Option<&str>,
) -> String {
    [title, url, text]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn share_status_draft(status: &str) -> std::result::Result<cfwdon_domain::StatusDraft, String> {
    ComposingStatus {
        text: status.to_owned(),
        visibility: Visibility::Public,
        spoiler_text: String::new(),
        sensitive: false,
        language: None,
        quote_approval_policy: None,
        in_reply_to_id: None,
        media_ids: Vec::new(),
        poll: None,
    }
    .validate(None)
    .map(|transition| transition.state)
    .map_err(|error| error.to_string())
}

async fn read_share_form(req: &mut Request) -> std::result::Result<ShareForm, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| error.to_string())?
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.contains("application/x-www-form-urlencoded")
        || content_type.contains("multipart/form-data")
    {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid share form: {error}"))?;
        return form
            .get_field("status")
            .map(|status| ShareForm { status })
            .ok_or_else(|| "status is required".to_owned());
    }

    req.json::<ShareForm>()
        .await
        .map_err(|error| format!("invalid share payload: {error}"))
}

fn share_login_redirect(config: &super::AppConfig, req: &Request) -> Result<Response> {
    let return_url = req.url()?;
    auth0_login_redirect_response(config, &return_url, &return_url)
}

fn share_document(initial_text: &str) -> String {
    share_page(
        "Share",
        &format!(
            r#"<p class="lead">Compose a public post on this server.</p>
<form method="post" action="/share">
<textarea name="status" rows="8" maxlength="500" required>{text}</textarea>
<button type="submit">Publish</button>
</form>"#,
            text = escape_html(initial_text),
        ),
    )
}

fn share_document_with_error(initial_text: &str, error: &str) -> String {
    share_page(
        "Share",
        &format!(
            r#"<p class="lead">Compose a public post on this server.</p>
<p class="error">{error}</p>
<form method="post" action="/share">
<textarea name="status" rows="8" maxlength="500" required>{text}</textarea>
<button type="submit">Publish</button>
</form>"#,
            error = escape_html(error),
            text = escape_html(initial_text),
        ),
    )
}

fn share_page(title: &str, body: &str) -> String {
    let title = escape_html(title);
    format!(
        r#"<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>
:root{{color-scheme:dark;--bg:#101114;--panel:#181b20;--line:#30343c;--text:#f4f0e8;--muted:#a9adb7;--accent:#45c08d;--ink:#0f1411;--danger:#ff7b72}}
*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;display:grid;place-items:center;background:#101114;color:var(--text);font-family:Inter,ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;line-height:1.5}}main{{width:min(560px,100%);padding:24px}}section{{border:1px solid var(--line);border-radius:8px;background:var(--panel);padding:28px}}h1{{margin:0 0 12px;font-size:32px;line-height:1.1}}.lead{{margin:0 0 16px;font-size:18px}}.error{{margin:0 0 12px;color:var(--danger)}}textarea{{width:100%;margin:0 0 16px;padding:12px;border-radius:8px;border:1px solid var(--line);background:#0f1217;color:var(--text);font:inherit;resize:vertical}}button{{display:inline-flex;align-items:center;justify-content:center;min-height:42px;padding:0 16px;border-radius:8px;border:1px solid var(--accent);background:var(--accent);color:var(--ink);font:inherit;font-weight:650;cursor:pointer}}
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

#[cfg(test)]
mod tests {
    use super::share_initial_text;

    #[test]
    fn share_initial_text_joins_title_url_and_text() {
        assert_eq!(
            share_initial_text(Some("Hello"), Some("https://example.com"), Some("more")),
            "Hello\n\nhttps://example.com\n\nmore"
        );
        assert_eq!(
            share_initial_text(None, None, Some("only text")),
            "only text"
        );
        assert_eq!(share_initial_text(Some("  "), Some(""), None), "");
    }
}
