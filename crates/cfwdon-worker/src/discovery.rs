use super::{
    AccountStats, AppConfig, Error, LocalAccount, Request, Response, Result, RouteContext,
    actor_url, build_activitypub_actor_document, build_outbox_activities, build_tag_response,
    cache_actor_json_response, cache_actor_profile_html_response, cached_actor_json_response,
    cached_actor_profile_html_response, ensure_account_keys, escape_html, find_account_by_username,
    instance_host, json_response, list_follower_actor_uris, list_following_actor_uris,
    list_local_follower_usernames, list_public_outbox_statuses, load_account_stats, load_config,
    media_object_url, normalize_hashtag, parse_webfinger_resource, render_profile_field_value_html,
};
use std::collections::HashSet;
use url::Url;
use worker::ResponseBody;

#[derive(Debug, serde::Deserialize)]
struct WebFingerQuery {
    resource: String,
}

#[derive(Debug, serde::Deserialize)]
struct RemoteFollowQuery {
    domain: String,
}

#[derive(Debug, serde::Serialize)]
struct WebFingerResponse {
    subject: String,
    links: Vec<WebFingerLink>,
}

#[derive(Debug, serde::Serialize)]
struct WebFingerLink {
    rel: &'static str,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    link_type: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    href: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    template: Option<String>,
}

impl WebFingerLink {
    fn self_link(href: String) -> Self {
        Self {
            rel: "self",
            link_type: Some("application/activity+json"),
            href: Some(href),
            template: None,
        }
    }

    fn subscribe_link(template: String) -> Self {
        Self {
            rel: "http://ostatus.org/schema/1.0/subscribe",
            link_type: None,
            href: None,
            template: Some(template),
        }
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct CollectionPagingQuery {
    page: Option<bool>,
    limit: Option<u32>,
    offset: Option<u32>,
}

pub(crate) async fn webfinger_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: WebFingerQuery = req.query()?;
    let handle = parse_webfinger_resource(&query.resource)?;

    if !handle.is_local_to(&config.instance_domain) {
        return Response::error("resource not found", 404);
    }

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &handle.username).await? else {
        return Response::error("resource not found", 404);
    };

    let instance_host = instance_host(&config);
    let response = WebFingerResponse {
        subject: format!("acct:{}@{}", account.username, instance_host),
        links: vec![
            WebFingerLink::self_link(actor_url(&config, &account.username)),
            WebFingerLink::subscribe_link(format!(
                "{}/remote-follow?domain={{uri}}",
                actor_url(&config, &account.username)
            )),
        ],
    };

    json_response(
        &response,
        "application/jrd+json",
        &[("Access-Control-Allow-Origin", "*")],
    )
}

pub(crate) async fn actor_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;

    let wants_html = prefers_profile_html(&req)?;
    if wants_html {
        if let Some(response) = cached_actor_profile_html_response(&ctx, &username).await? {
            return Ok(response);
        }
    } else if let Some(response) = cached_actor_json_response(&ctx, &username).await? {
        return Ok(response);
    }

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let account = ensure_account_keys(&db, account).await?;

    if wants_html {
        let stats = load_account_stats(&db, &account.id).await?;
        let html = profile_html_document(&config, &account, &stats);
        cache_actor_profile_html_response(&ctx, &username, html.clone()).await?;
        return profile_html_response(html);
    }

    let response = build_activitypub_actor_document(&config, &account);
    cache_actor_json_response(&ctx, &username, &response).await?;

    json_response(&response, "application/activity+json", &[])
}

pub(crate) async fn remote_follow_response(
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
        return Response::error("actor not found", 404);
    };
    let query: RemoteFollowQuery = req.query()?;
    let remote_base = remote_follow_base_url(&query.domain)?;
    let acct = format!("acct:{}@{}", account.username, instance_host(&config));
    let location = format!(
        "{}/authorize_interaction?uri={}",
        remote_base.trim_end_matches('/'),
        urlencoding::encode(&acct)
    );
    redirect_response(&location)
}

fn prefers_profile_html(req: &Request) -> Result<bool> {
    let accept = req.headers().get("Accept")?.unwrap_or_default();
    let accept = accept.to_ascii_lowercase();
    Ok(accept.contains("text/html")
        && !accept.contains("application/activity+json")
        && !accept.contains("application/ld+json"))
}

pub(crate) fn remote_follow_base_url(domain: &str) -> Result<String> {
    let domain = remote_follow_host(domain)?;
    let url = Url::parse(&format!("https://{domain}"))
        .map_err(|error| Error::RustError(format!("invalid remote follow domain: {error}")))?;
    Ok(url.to_string())
}

fn remote_follow_host(input: &str) -> Result<String> {
    let input = input.trim().trim_end_matches('/');
    let domain = if input
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("acct:"))
    {
        let handle = parse_webfinger_resource(input)?;
        handle.domain.unwrap_or_default()
    } else if let Some(url) = input
        .contains("://")
        .then(|| Url::parse(input))
        .transpose()
        .map_err(|error| Error::RustError(format!("invalid remote follow URL: {error}")))?
    {
        url.host_str().unwrap_or_default().to_owned()
    } else if !input.contains('/') {
        if let Some((_, domain)) = input.trim_start_matches('@').rsplit_once('@') {
            domain.to_owned()
        } else {
            input.to_owned()
        }
    } else {
        input.to_owned()
    };
    let domain = domain.trim().trim_end_matches('/').to_ascii_lowercase();
    if domain.is_empty() {
        return Err(Error::RustError(
            "remote follow domain is required".to_owned(),
        ));
    }
    if domain.contains('/') || domain.contains('?') || domain.contains('#') {
        return Err(Error::RustError(
            "remote follow domain must be a hostname".to_owned(),
        ));
    }
    if Url::parse(&format!("https://{domain}"))
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_none()
    {
        return Err(Error::RustError(
            "remote follow domain must include a hostname".to_owned(),
        ));
    }
    Ok(domain)
}

fn redirect_response(location: &str) -> Result<Response> {
    let escaped = escape_html(location);
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"0;url={escaped}\"><title>Redirecting</title></head><body><main><p>Redirecting to <a href=\"{escaped}\">{escaped}</a>.</p></main></body></html>"
    );
    let mut response = Response::from_body(ResponseBody::Body(body.into_bytes()))?.with_status(302);
    response.headers_mut().set("Location", location)?;
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    response.headers_mut().set("Cache-Control", "no-store")?;
    Ok(response)
}

fn profile_html_document(
    config: &AppConfig,
    account: &LocalAccount,
    stats: &AccountStats,
) -> String {
    let profile_url = actor_url(config, &account.username);
    let display_name_source = if account.display_name.trim().is_empty() {
        format!("@{}", account.username)
    } else {
        account.display_name.clone()
    };
    let display_name = escape_html(&display_name_source);
    let username = escape_html(&format!("@{}@{}", account.username, instance_host(config)));
    let title = escape_html(&format!("{display_name_source} ({})", account.username));
    let avatar_url = account
        .avatar_object_key
        .as_deref()
        .map(|object_key| media_object_url(config, object_key));
    let header_url = account
        .header_object_key
        .as_deref()
        .map(|object_key| media_object_url(config, object_key));
    let header_style = header_url
        .as_ref()
        .map(|url| format!("background-image:url('{}')", css_single_quoted_value(url)))
        .unwrap_or_default();
    let avatar_html = avatar_url
        .as_ref()
        .map(|url| {
            format!(
                "<img class=\"avatar\" src=\"{}\" alt=\"{}\" loading=\"lazy\">",
                escape_html(url),
                display_name
            )
        })
        .unwrap_or_else(|| {
            format!(
                "<div class=\"avatar avatar-fallback\">{}</div>",
                profile_initial(&display_name_source)
            )
        });
    let bio_html = if account.bio_html.trim().is_empty() {
        "<p class=\"muted\">No profile note yet.</p>".to_owned()
    } else {
        account.bio_html.clone()
    };
    let fields_html = if account.fields.is_empty() {
        String::new()
    } else {
        format!(
            "<dl class=\"fields\">{}</dl>",
            account
                .fields
                .iter()
                .map(|field| {
                    format!(
                        "<div><dt>{}</dt><dd>{}</dd></div>",
                        escape_html(&field.name),
                        render_profile_field_value_html(&field.value)
                    )
                })
                .collect::<Vec<_>>()
                .join("")
        )
    };
    let lock_badge = account
        .locked
        .then_some("<span class=\"badge\">Locked</span>")
        .unwrap_or("");
    let bot_badge = account
        .bot
        .then_some("<span class=\"badge\">Bot</span>")
        .unwrap_or("");
    let created = escape_html(&account.created_at);
    format!(
        r#"<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<meta name="description" content="{username} on {instance}">
<link rel="alternate" type="application/activity+json" href="{profile_url}">
<style>
:root{{color-scheme:dark;--bg:#101114;--panel:#181b20;--line:#30343c;--text:#f4f0e8;--muted:#a9adb7;--accent:#45c08d;--accent-2:#f2b84b;--ink:#0f1411}}
*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;background:radial-gradient(circle at 12% 8%,#263d34 0 18rem,transparent 18.5rem),linear-gradient(135deg,#101114 0%,#171a1f 56%,#221f1a 100%);color:var(--text);font-family:Inter,ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;line-height:1.5}}
a{{color:inherit}}main{{width:min(960px,100%);margin:0 auto;padding:32px 20px 48px}}.shell{{overflow:hidden;border:1px solid var(--line);border-radius:8px;background:rgba(24,27,32,.92);box-shadow:0 24px 80px rgba(0,0,0,.32)}}.cover{{height:260px;background:linear-gradient(120deg,#224334,#665126);background-size:cover;background-position:center;{header_style}}}.profile{{display:grid;grid-template-columns:auto 1fr;gap:22px;padding:0 28px 28px}}.avatar{{width:132px;height:132px;margin-top:-66px;border:4px solid var(--panel);border-radius:8px;object-fit:cover;background:#222831}}.avatar-fallback{{display:grid;place-items:center;background:linear-gradient(135deg,var(--accent),var(--accent-2));color:var(--ink);font-size:56px;font-weight:800}}.identity{{padding-top:18px}}h1{{margin:0;font-size:clamp(34px,5vw,58px);line-height:1.02;letter-spacing:0}}.handle{{margin:8px 0 0;color:var(--muted);font-size:16px}}.badges{{display:flex;gap:8px;flex-wrap:wrap;margin-top:14px}}.badge{{border:1px solid #49505a;border-radius:999px;padding:4px 10px;color:#d6dae0;font-size:13px}}.note{{padding:0 28px 28px;font-size:18px}}.note p{{margin:0 0 1em}}.muted{{color:var(--muted)}}.fields{{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:12px;margin:2px 0 28px;padding:0 28px}}.fields div{{border:1px solid var(--line);border-radius:8px;padding:12px;background:#13161b}}dt{{color:var(--muted);font-size:13px}}dd{{margin:4px 0 0;overflow-wrap:anywhere}}.stats{{display:grid;grid-template-columns:repeat(3,1fr);border-top:1px solid var(--line)}}.stats div{{padding:20px 28px;border-right:1px solid var(--line)}}.stats div:last-child{{border-right:0}}.num{{display:block;font-size:28px;font-weight:750}}.label{{color:var(--muted);font-size:13px;text-transform:uppercase;letter-spacing:.08em}}.actions{{display:flex;gap:12px;flex-wrap:wrap;padding:24px 28px;border-top:1px solid var(--line)}}.button{{display:inline-flex;align-items:center;justify-content:center;min-height:42px;padding:0 16px;border-radius:8px;border:1px solid var(--line);text-decoration:none;font-weight:650}}.primary{{background:var(--accent);border-color:var(--accent);color:var(--ink)}}.remote-follow{{display:flex;gap:8px;flex-wrap:wrap;align-items:center}}.remote-follow input{{min-height:42px;width:220px;max-width:100%;border:1px solid var(--line);border-radius:8px;background:#101318;color:var(--text);padding:0 12px;font:inherit}}footer{{margin-top:18px;color:var(--muted);font-size:13px;text-align:center}}@media (max-width:640px){{main{{padding:12px}}.cover{{height:190px}}.profile{{grid-template-columns:1fr;padding:0 18px 20px}}.avatar{{width:112px;height:112px;margin-top:-56px}}.identity{{padding-top:0}}.fields,.stats{{grid-template-columns:1fr}}.fields{{padding:0 18px 20px}}.note{{padding:0 18px 20px}}.stats div{{border-right:0;border-bottom:1px solid var(--line)}}.stats div:last-child{{border-bottom:0}}.actions{{padding:20px 18px}}}}
</style>
</head>
<body>
<main>
<section class="shell">
<div class="cover" aria-hidden="true"></div>
<div class="profile">{avatar_html}<div class="identity"><h1>{display_name}</h1><p class="handle">{username}</p><div class="badges">{lock_badge}{bot_badge}</div></div></div>
<div class="note">{bio_html}</div>
{fields_html}
<div class="stats"><div><span class="num">{statuses}</span><span class="label">Posts</span></div><div><span class="num">{followers}</span><span class="label">Followers</span></div><div><span class="num">{following}</span><span class="label">Following</span></div></div>
<div class="actions"><a class="button" href="{profile_url}/statuses">Public posts</a><form class="remote-follow" action="{profile_url}/remote-follow" method="get"><input name="domain" inputmode="url" autocomplete="url" placeholder="your.server or @you@server" aria-label="Your home server domain or handle" required><button class="button primary" type="submit">Remote follow</button></form></div>
</section>
<footer>Joined {created}</footer>
</main>
</body>
</html>"#,
        followers = stats.followers_count,
        following = stats.following_count,
        instance = escape_html(&config.instance_name),
        statuses = stats.statuses_count,
    )
}

fn profile_html_response(html: String) -> Result<Response> {
    let mut response = Response::from_body(ResponseBody::Body(html.into_bytes()))?;
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    Ok(response)
}

fn profile_initial(value: &str) -> String {
    value
        .trim_start_matches('@')
        .chars()
        .next()
        .map(|value| escape_html(&value.to_string()))
        .unwrap_or_else(|| "@".to_owned())
}

fn css_single_quoted_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

pub(crate) async fn tag_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let tag = ctx
        .param("name")
        .or_else(|| ctx.param("hashtag"))
        .map(|value| normalize_hashtag(value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing tag route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;

    Response::from_json(&build_tag_response(&db, &config, &tag).await?)
}

pub(crate) async fn followers_collection_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: CollectionPagingQuery = req.query().unwrap_or_default();
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let mut ordered_items = list_follower_actor_uris(&db, &account.id).await?;
    let mut seen = ordered_items.iter().cloned().collect::<HashSet<_>>();
    for username in list_local_follower_usernames(&db, &account.id).await? {
        let actor_uri = actor_url(&config, &username);
        if seen.insert(actor_uri.clone()) {
            ordered_items.push(actor_uri);
        }
    }
    let collection_id = format!("{}/followers", actor_url(&config, &account.username));
    json_response(
        &build_ordered_collection_document(&collection_id, &ordered_items, &query),
        "application/activity+json",
        &[],
    )
}

pub(crate) async fn following_collection_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: CollectionPagingQuery = req.query().unwrap_or_default();
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let ordered_items = list_following_actor_uris(&db, &account.id).await?;
    let collection_id = format!("{}/following", actor_url(&config, &account.username));

    json_response(
        &build_ordered_collection_document(&collection_id, &ordered_items, &query),
        "application/activity+json",
        &[],
    )
}

pub(crate) async fn outbox_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };

    let statuses = list_public_outbox_statuses(&db, &account.id, 20).await?;
    let actor = actor_url(&config, &account.username);
    let outbox = format!("{actor}/outbox");
    let ordered_items = build_outbox_activities(&db, &config, &account, &statuses).await?;

    json_response(
        &serde_json::json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "type": "OrderedCollection",
            "id": outbox,
            "totalItems": ordered_items.len(),
            "orderedItems": ordered_items,
        }),
        "application/activity+json",
        &[],
    )
}

fn build_ordered_collection_document(
    collection_id: &str,
    ordered_items: &[String],
    query: &CollectionPagingQuery,
) -> serde_json::Value {
    let total_items = ordered_items.len();
    let limit = query.limit.unwrap_or(50).clamp(1, 80) as usize;
    let offset = query.offset.unwrap_or(0) as usize;

    if query.page.unwrap_or(false) || query.offset.unwrap_or(0) > 0 {
        let page_items = ordered_items
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_offset = offset.saturating_add(page_items.len());
        let next = if next_offset < total_items {
            Some(format!(
                "{collection_id}?page=true&offset={next_offset}&limit={limit}"
            ))
        } else {
            None
        };

        serde_json::json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "type": "OrderedCollectionPage",
            "id": format!("{collection_id}?page=true&offset={offset}&limit={limit}"),
            "partOf": collection_id,
            "next": next,
            "orderedItems": page_items,
        })
    } else {
        serde_json::json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "type": "OrderedCollection",
            "id": collection_id,
            "totalItems": total_items,
            "first": format!("{collection_id}?page=true&offset=0&limit={limit}"),
        })
    }
}
