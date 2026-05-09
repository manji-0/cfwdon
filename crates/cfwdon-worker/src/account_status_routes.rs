use super::{
    AccountReference, AccountStatusesQuery, AppConfig, Error, LocalAccount, RemoteActorRow,
    RemoteStatusRow, Request, Response, Result, RouteContext, StatusRow, actor_url,
    build_local_status_response, build_remote_status_response, can_view_local_status, escape_html,
    find_account_by_username, find_authenticated_local_account,
    find_media_attachments_by_status_id, is_public_activitypub_visibility, list_account_statuses,
    list_pinned_statuses_for_account, list_remote_statuses_by_actor_uri, load_config,
    load_in_reply_to_account_id, local_status_ap_id, remote_status_has_media,
    resolve_account_reference, status_contains_tag, status_is_reply_to_other_account,
    strip_html_tags,
};
use worker::ResponseBody;

pub(crate) async fn account_statuses_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;
    account_statuses_response_for_account_id(req, ctx, account_id).await
}

pub(crate) async fn account_statuses_by_username_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("account not found", 404);
    };
    let account_id = account.id;
    drop(db);

    account_statuses_response_for_account_id(req, ctx, account_id).await
}

async fn account_statuses_response_for_account_id(
    req: Request,
    ctx: RouteContext<()>,
    account_id: String,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountStatusesQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let wants_html = prefers_statuses_html(&req)?;

    let db = ctx.d1(&config.database_binding)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    match resolve_account_reference(&db, &account_id).await? {
        Some(AccountReference::Local(account)) => {
            let statuses = if query.pinned.unwrap_or(false) {
                list_pinned_statuses_for_account(&db, &account.id).await?
            } else {
                list_account_statuses(&db, &account.id, limit).await?
            };
            let mut response = Vec::new();
            let mut html_statuses = Vec::new();

            for status in statuses.into_iter().take(limit as usize) {
                if !can_view_local_status(&db, &status, viewer.as_ref(), &account).await? {
                    continue;
                }
                if let Some(tag) = query.tagged.as_deref()
                    && !status_contains_tag(&status, tag)
                {
                    continue;
                }
                if query.exclude_reblogs.unwrap_or(false) {
                    if status.boost_of_uri.is_some() {
                        continue;
                    }
                }
                if query.exclude_replies.unwrap_or(false)
                    && status_is_reply_to_other_account(&db, &status, &account.id).await?
                {
                    continue;
                }

                let media = find_media_attachments_by_status_id(&db, &status.id).await?;
                if query.only_media.unwrap_or(false) && media.is_empty() {
                    continue;
                }

                if wants_html {
                    html_statuses.push(local_status_html_item(&config, &account, &status));
                }
                response.push(
                    build_local_status_response(
                        &db,
                        &config,
                        viewer.as_ref(),
                        &status,
                        &account,
                        load_in_reply_to_account_id(&db, &status).await?,
                        media,
                    )
                    .await?,
                );
            }

            if wants_html {
                return account_statuses_html_response(
                    &config,
                    &account.display_name,
                    &account.username,
                    &actor_url(&config, &account.username),
                    &html_statuses,
                );
            }

            Response::from_json(&response)
        }
        Some(AccountReference::Remote(actor)) => {
            let mut response = Vec::new();
            let mut html_statuses = Vec::new();
            for status in list_remote_statuses_by_actor_uri(&db, &actor.actor_uri, limit).await? {
                if !is_public_activitypub_visibility(&status.visibility) {
                    continue;
                }
                if query.pinned.unwrap_or(false) {
                    continue;
                }
                if let Some(tag) = query.tagged.as_deref()
                    && !status
                        .content_html
                        .to_ascii_lowercase()
                        .contains(&tag.to_ascii_lowercase())
                {
                    continue;
                }
                if query.exclude_reblogs.unwrap_or(false) && status.boost_of_uri.is_some() {
                    continue;
                }
                if query.exclude_replies.unwrap_or(false) && status.in_reply_to_uri.is_some() {
                    continue;
                }
                if query.only_media.unwrap_or(false)
                    && !remote_status_has_media(&db, &status.id).await?
                {
                    continue;
                }

                if wants_html {
                    html_statuses.push(remote_status_html_item(&actor, &status));
                }
                response.push(
                    build_remote_status_response(&db, &config, viewer.as_ref(), &status, &actor)
                        .await?,
                );
            }
            if wants_html {
                let profile_url = actor
                    .profile_url
                    .clone()
                    .unwrap_or_else(|| actor.actor_uri.clone());
                return account_statuses_html_response(
                    &config,
                    &actor.display_name,
                    &format!("{}@{}", actor.username, actor.domain),
                    &profile_url,
                    &html_statuses,
                );
            }
            Response::from_json(&response)
        }
        None => Response::error("account not found", 404),
    }
}

fn prefers_statuses_html(req: &Request) -> Result<bool> {
    let accept = req.headers().get("Accept")?.unwrap_or_default();
    let accept = accept.to_ascii_lowercase();
    Ok(accept.contains("text/html") && !accept.contains("application/json"))
}

fn account_statuses_html_response(
    config: &AppConfig,
    display_name: &str,
    handle: &str,
    profile_url: &str,
    statuses: &[String],
) -> Result<Response> {
    let name = if display_name.trim().is_empty() {
        handle.to_owned()
    } else {
        display_name.to_owned()
    };
    let title = escape_html(&format!("{name} posts"));
    let name = escape_html(&name);
    let handle = escape_html(&format!("@{handle}"));
    let profile_url = escape_html(profile_url);
    let instance_name = escape_html(&config.instance_name);
    let statuses_html = if statuses.is_empty() {
        "<p class=\"empty\">No public posts found.</p>".to_owned()
    } else {
        statuses.join("")
    };
    let html = format!(
        r#"<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>
:root{{color-scheme:dark;--bg:#101114;--panel:#181b20;--line:#30343c;--text:#f4f0e8;--muted:#a9adb7;--accent:#45c08d;--accent-2:#f2b84b;--ink:#0f1411}}
*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;background:linear-gradient(135deg,#101114 0%,#171a1f 58%,#1f241e 100%);color:var(--text);font-family:Inter,ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;line-height:1.55}}a{{color:inherit}}main{{width:min(820px,100%);margin:0 auto;padding:32px 20px 56px}}header{{display:flex;align-items:flex-end;justify-content:space-between;gap:20px;margin-bottom:24px}}h1{{margin:0;font-size:clamp(34px,6vw,56px);line-height:1.03;letter-spacing:0}}.handle,.meta,.empty,time{{color:var(--muted)}}.nav{{display:flex;gap:10px;flex-wrap:wrap}}.button{{display:inline-flex;align-items:center;justify-content:center;min-height:42px;padding:0 16px;border:1px solid var(--line);border-radius:8px;text-decoration:none;font-weight:650}}.primary{{background:var(--accent);border-color:var(--accent);color:var(--ink)}}.feed{{display:grid;gap:14px}}article{{border:1px solid var(--line);border-radius:8px;background:rgba(24,27,32,.92);padding:20px;box-shadow:0 18px 54px rgba(0,0,0,.22)}}.content{{font-size:18px;overflow-wrap:anywhere}}.content p:first-child{{margin-top:0}}.content p:last-child{{margin-bottom:0}}.spoiler{{margin:0 0 12px;color:var(--accent-2);font-weight:700}}time{{display:block;margin-top:16px;font-size:13px}}.empty{{border:1px solid var(--line);border-radius:8px;padding:24px;background:rgba(24,27,32,.92)}}footer{{margin-top:20px;color:var(--muted);font-size:13px;text-align:center}}@media (max-width:640px){{main{{padding:18px 12px 42px}}header{{display:block}}.nav{{margin-top:18px}}article{{padding:16px}}}}
</style>
</head>
<body>
<main>
<header><div><p class="meta">{instance_name}</p><h1>{name}</h1><p class="handle">{handle}</p></div><nav class="nav"><a class="button primary" href="{profile_url}">Profile</a></nav></header>
<section class="feed">{statuses_html}</section>
<footer>Public posts</footer>
</main>
</body>
</html>"#
    );
    let mut response = Response::from_body(ResponseBody::Body(html.into_bytes()))?;
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    Ok(response)
}

fn local_status_html_item(
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> String {
    let status_url = escape_html(&local_status_ap_id(config, account, status));
    status_html_item(
        &status_url,
        &status.content_html,
        &status.spoiler_text,
        &status.created_at,
    )
}

fn remote_status_html_item(actor: &RemoteActorRow, status: &RemoteStatusRow) -> String {
    let status_url = escape_html(status.url.as_deref().unwrap_or(status.object_uri.as_str()));
    let _actor = actor;
    status_html_item(
        &status_url,
        &status.content_html,
        &status.spoiler_text,
        &status.published_at,
    )
}

fn status_html_item(
    url: &str,
    content_html: &str,
    spoiler_text: &str,
    published_at: &str,
) -> String {
    let spoiler = if spoiler_text.trim().is_empty() {
        String::new()
    } else {
        format!("<p class=\"spoiler\">{}</p>", escape_html(spoiler_text))
    };
    let plain = strip_html_tags(content_html);
    let aria_label = if plain.trim().is_empty() {
        "Open post".to_owned()
    } else {
        plain
    };
    format!(
        "<article><a href=\"{url}\" aria-label=\"{}\"><div class=\"content\">{}{}</div><time>{}</time></a></article>",
        escape_html(&aria_label),
        spoiler,
        content_html,
        escape_html(published_at)
    )
}
