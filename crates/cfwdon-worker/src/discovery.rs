use super::{
    AccountStats, AppConfig, CACHE_TTL_FEDERATION, CACHE_TTL_STATIC_METADATA, CACHE_TTL_TRENDS,
    Error, LocalAccount, Request, Response, Result, RouteContext, account_profile_page_url,
    activitypub_datetime_string, actor_url, authorize_interaction_object_template,
    authorize_interaction_subscribe_template, build_activitypub_actor_document,
    build_outbox_activities, build_tag_response, cache_actor_json_response,
    cache_actor_profile_html_response, cache_public_json_response, cache_public_response,
    cache_public_response_with_options, cached_actor_json_response,
    cached_actor_profile_html_response, count_public_outbox_statuses, ensure_account_keys,
    escape_html, find_account_by_username, find_media_attachments_by_status_ids, instance_base_url,
    instance_host, list_follower_actor_uris, list_following_actor_uris,
    list_local_follower_usernames, list_public_outbox_statuses, list_public_outbox_statuses_page,
    load_account_stats, load_config, local_status_html_item, media_object_url, normalize_hashtag,
    parse_webfinger_resource, render_profile_field_value_html, share_create_template,
    webfinger_lrdd_template,
};
use std::collections::HashSet;
use url::Url;
use worker::ResponseBody;

#[derive(Debug, serde::Deserialize)]
struct RemoteFollowQuery {
    domain: String,
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedWebFingerQuery {
    resource: String,
    rels: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
struct WebFingerResponse {
    subject: String,
    aliases: Vec<String>,
    links: Vec<WebFingerLink>,
}

#[derive(Debug, serde::Serialize)]
struct WebFingerLink {
    rel: &'static str,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    link_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    href: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    template: Option<String>,
}

impl WebFingerLink {
    fn profile_page_link(href: String) -> Self {
        Self {
            rel: "http://webfinger.net/rel/profile-page",
            link_type: Some("text/html".to_owned()),
            href: Some(href),
            template: None,
        }
    }

    fn self_link(href: String) -> Self {
        Self {
            rel: "self",
            link_type: Some("application/activity+json".to_owned()),
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

    fn create_intent_link(template: String) -> Self {
        Self {
            rel: "https://w3id.org/fep/3b86/Create",
            link_type: None,
            href: None,
            template: Some(template),
        }
    }

    fn object_intent_link(template: String) -> Self {
        Self {
            rel: "https://w3id.org/fep/3b86/Object",
            link_type: None,
            href: None,
            template: Some(template),
        }
    }

    fn avatar_link(href: String, media_type: &str) -> Self {
        Self {
            rel: "http://webfinger.net/rel/avatar",
            link_type: Some(media_type.to_owned()),
            href: Some(href),
            template: None,
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
    let query = match parse_webfinger_query_pairs(req.url()?.query_pairs()) {
        Ok(query) => query,
        Err(message) => return Response::error(message, 400),
    };
    let handle = match parse_webfinger_resource(&query.resource) {
        Ok(handle) => handle,
        Err(error) => return Response::error(error.to_string(), 400),
    };

    if !handle.is_local_to(&config.instance_domain) {
        return Response::error("resource not found", 404);
    }

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &handle.username).await? else {
        return Response::error("resource not found", 404);
    };

    let instance_host = instance_host(&config);
    let username = account.username();
    let actor = actor_url(&config, username);
    let profile_page = account_profile_page_url(&config, username);
    let mut links = vec![
        WebFingerLink::profile_page_link(profile_page.clone()),
        WebFingerLink::self_link(actor.clone()),
        WebFingerLink::subscribe_link(authorize_interaction_subscribe_template(&config)),
        WebFingerLink::create_intent_link(share_create_template(&config)),
        WebFingerLink::object_intent_link(authorize_interaction_object_template(&config)),
    ];
    if let Some((object_key, content_type)) = account
        .avatar_object_key()
        .zip(account.avatar_content_type())
    {
        links.push(WebFingerLink::avatar_link(
            media_object_url(&config, object_key),
            content_type,
        ));
    }
    let links = filter_webfinger_links(links, &query.rels);
    let response = WebFingerResponse {
        subject: format!("acct:{username}@{instance_host}"),
        aliases: vec![profile_page, actor],
        links,
    };

    // CORS is applied centrally via `is_cors_enabled_path("/.well-known/webfinger")`.
    cache_public_json_response(
        &response,
        "application/jrd+json",
        CACHE_TTL_STATIC_METADATA,
        &[],
    )
}

/// Parse WebFinger query pairs per RFC 7033 §4.1.
///
/// `resource` is required once (last value wins if repeated). Zero or more `rel`
/// values select link relations; when present, unmatched links are omitted.
fn parse_webfinger_query_pairs<I, K, V>(
    pairs: I,
) -> std::result::Result<ParsedWebFingerQuery, String>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut resource = None;
    let mut rels = Vec::new();
    for (key, value) in pairs {
        match key.as_ref() {
            "resource" => resource = Some(value.as_ref().to_owned()),
            "rel" => {
                let value = value.as_ref();
                if !value.is_empty() {
                    rels.push(value.to_owned());
                }
            }
            _ => {}
        }
    }
    let resource = resource
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "resource query parameter is required".to_owned())?;
    Ok(ParsedWebFingerQuery { resource, rels })
}

fn filter_webfinger_links(links: Vec<WebFingerLink>, rels: &[String]) -> Vec<WebFingerLink> {
    if rels.is_empty() {
        return links;
    }
    links
        .into_iter()
        .filter(|link| rels.iter().any(|rel| rel == link.rel))
        .collect()
}

pub(crate) async fn host_meta_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    host_meta_response_for_config(&config)
}

pub(crate) fn host_meta_response_from_env(env: &worker::Env) -> Result<Response> {
    let config = crate::load_config_from_env(env);
    host_meta_response_for_config(&config)
}

fn host_meta_response_for_config(config: &AppConfig) -> Result<Response> {
    let template = webfinger_lrdd_template(config);
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <XRD xmlns=\"http://docs.oasis-open.org/ns/xri/xrd-1.0\">\n\
           <Link rel=\"lrdd\" template=\"{}\"/>\n\
         </XRD>\n",
        escape_xml_attr(&template)
    );
    let mut response = Response::from_body(ResponseBody::Body(body.into_bytes()))?;
    response
        .headers_mut()
        .set("Content-Type", "application/xrd+xml; charset=utf-8")?;
    response
        .headers_mut()
        .set("Access-Control-Allow-Origin", "*")?;
    cache_public_response(response, CACHE_TTL_STATIC_METADATA)
}

fn escape_xml_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) async fn actor_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let username = route_username(&ctx)?;

    let wants_html = prefers_profile_html(&req)?;
    if wants_html {
        if let Some(response) = cached_actor_profile_html_response(&ctx, &username).await? {
            return with_profile_discovery_link_headers(response, &config, &username);
        }
    } else if let Some(response) = cached_actor_json_response(&ctx, &username).await? {
        return Ok(response);
    }

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let account = ensure_account_keys(&db, &config, account).await?;

    if wants_html {
        let stats = load_account_stats(&db, account.id()).await?;
        let statuses = list_public_outbox_statuses(&db, account.id(), 20).await?;
        let status_ids = statuses
            .iter()
            .map(|status| status.id.clone())
            .collect::<Vec<_>>();
        let mut media_by_status_id = find_media_attachments_by_status_ids(&db, &status_ids).await?;
        let posts_html = statuses
            .iter()
            .map(|status| {
                let media = media_by_status_id.remove(&status.id).unwrap_or_default();
                local_status_html_item(&config, &account, status, &media)
            })
            .collect::<Vec<_>>()
            .join("");
        let html = profile_html_document(&config, &account, &stats, &posts_html);
        cache_actor_profile_html_response(&ctx, &username, html.clone()).await?;
        let cache_tag = format!("account-{username}");
        let response = cache_public_response_with_options(
            profile_html_response(html)?,
            CACHE_TTL_FEDERATION,
            None,
            &[("Cache-Tag", &cache_tag)],
        )?;
        return with_profile_discovery_link_headers(response, &config, &username);
    }

    let response = build_activitypub_actor_document(&config, &account);
    cache_actor_json_response(&ctx, &username, &response).await?;

    let cache_tag = format!("account-{username}");
    cache_public_json_response(
        &response,
        "application/activity+json",
        CACHE_TTL_FEDERATION,
        &[("Vary", "Accept"), ("Cache-Tag", &cache_tag)],
    )
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
    let acct = format!("acct:{}@{}", account.username(), instance_host(&config));
    let location = format!(
        "{}/authorize_interaction?uri={}",
        remote_base.trim_end_matches('/'),
        urlencoding::encode(&acct)
    );
    redirect_response(&location)
}

fn route_username(ctx: &RouteContext<()>) -> Result<String> {
    ctx.param("username")
        .map(|value| value.trim().trim_start_matches('@').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))
}

fn with_profile_discovery_link_headers(
    mut response: Response,
    config: &AppConfig,
    username: &str,
) -> Result<Response> {
    let acct = format!("acct:{}@{}", username, instance_host(config));
    let webfinger = format!(
        "{}/.well-known/webfinger?resource={}",
        instance_base_url(config),
        urlencoding::encode(&acct)
    );
    let actor = actor_url(config, username);
    let link = format!(
        "<{webfinger}>; rel=\"lrdd\"; type=\"application/jrd+json\", <{actor}>; rel=\"alternate\"; type=\"application/activity+json\""
    );
    response.headers_mut().set("Link", &link)?;
    if response.headers().get("Vary")?.is_none() {
        response.headers_mut().set("Vary", "Accept")?;
    }
    Ok(response)
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
    posts_html: &str,
) -> String {
    let profile_url = actor_url(config, account.username());
    let display_name_source = profile_display_name_source(account);
    let display_name = escape_html(&display_name_source);
    let username = escape_html(&format!(
        "@{}@{}",
        account.username(),
        instance_host(config)
    ));
    let title = escape_html(&format!("{display_name_source} ({})", account.username()));
    let avatar_url = account
        .avatar_object_key()
        .map(|object_key| media_object_url(config, object_key));
    let header_url = account
        .header_object_key()
        .map(|object_key| media_object_url(config, object_key));
    let header_style = profile_header_style(header_url.as_deref());
    let avatar_html =
        profile_avatar_html(&display_name_source, &display_name, avatar_url.as_deref());
    let bio_html = profile_bio_html(account);
    let fields_html = profile_fields_html(account);
    let badges_html = profile_badges_html(account);
    let created = escape_html(&activitypub_datetime_string(account.created_at()));
    let posts_section = profile_posts_section(&profile_url, posts_html);
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
a{{color:inherit}}main{{width:min(960px,100%);margin:0 auto;padding:32px 20px 48px}}.shell{{overflow:hidden;border:1px solid var(--line);border-radius:8px;background:rgba(24,27,32,.92);box-shadow:0 24px 80px rgba(0,0,0,.32)}}.cover{{height:260px;background:linear-gradient(120deg,#224334,#665126);background-size:cover;background-position:center;{header_style}}}.profile{{display:grid;grid-template-columns:auto 1fr;gap:22px;padding:0 28px 28px}}.avatar{{width:132px;height:132px;margin-top:-66px;border:4px solid var(--panel);border-radius:8px;object-fit:cover;background:#222831}}.avatar-fallback{{display:grid;place-items:center;background:linear-gradient(135deg,var(--accent),var(--accent-2));color:var(--ink);font-size:56px;font-weight:800}}.identity{{padding-top:18px}}h1{{margin:0;font-size:clamp(34px,5vw,58px);line-height:1.02;letter-spacing:0}}.handle{{margin:8px 0 0;color:var(--muted);font-size:16px}}.badges{{display:flex;gap:8px;flex-wrap:wrap;margin-top:14px}}.badge{{border:1px solid #49505a;border-radius:999px;padding:4px 10px;color:#d6dae0;font-size:13px}}.profile-actions{{display:flex;gap:12px;flex-wrap:wrap;margin-top:16px}}.note{{padding:0 28px 28px;font-size:18px}}.note p{{margin:0 0 1em}}.muted{{color:var(--muted)}}.fields{{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:12px;margin:2px 0 28px;padding:0 28px}}.fields div{{border:1px solid var(--line);border-radius:8px;padding:12px;background:#13161b}}dt{{color:var(--muted);font-size:13px}}dd{{margin:4px 0 0;overflow-wrap:anywhere}}.stats{{display:grid;grid-template-columns:repeat(3,1fr);border-top:1px solid var(--line)}}.stats div{{padding:20px 28px;border-right:1px solid var(--line)}}.stats div:last-child{{border-right:0}}.num{{display:block;font-size:28px;font-weight:750}}.label{{color:var(--muted);font-size:13px;text-transform:uppercase;letter-spacing:.08em}}.actions{{display:flex;gap:12px;flex-wrap:wrap;padding:24px 28px;border-top:1px solid var(--line)}}.button{{display:inline-flex;align-items:center;justify-content:center;min-height:42px;padding:0 16px;border-radius:8px;border:1px solid var(--line);text-decoration:none;font-weight:650}}.primary{{background:var(--accent);border-color:var(--accent);color:var(--ink)}}.remote-follow{{display:flex;gap:8px;flex-wrap:wrap;align-items:center}}.remote-follow input{{min-height:42px;width:220px;max-width:100%;border:1px solid var(--line);border-radius:8px;background:#101318;color:var(--text);padding:0 12px;font:inherit}}.posts{{margin-top:18px}}.posts-header{{display:flex;align-items:center;justify-content:space-between;gap:16px;margin:0 0 12px}}.posts-header h2{{margin:0;font-size:24px;letter-spacing:0}}.posts-header a{{color:var(--muted);font-weight:650;text-decoration:none}}.feed{{display:grid;gap:14px}}article{{border:1px solid var(--line);border-radius:8px;background:rgba(24,27,32,.92);box-shadow:0 18px 54px rgba(0,0,0,.22)}}article a{{display:block;padding:20px;text-decoration:none}}.content{{font-size:18px;overflow-wrap:anywhere}}.content p:first-child{{margin-top:0}}.content p:last-child{{margin-bottom:0}}.spoiler{{margin:0 0 12px;color:var(--accent-2);font-weight:700}}.media{{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:8px;margin-top:14px}}.media img{{display:block;width:100%;max-height:360px;object-fit:cover;border-radius:8px}}time{{display:block;margin-top:16px;color:var(--muted);font-size:13px}}footer{{margin-top:18px;color:var(--muted);font-size:13px;text-align:center}}@media (max-width:640px){{main{{padding:12px}}.cover{{height:190px}}.profile{{grid-template-columns:1fr;padding:0 18px 20px}}.avatar{{width:112px;height:112px;margin-top:-56px}}.identity{{padding-top:0}}.fields,.stats{{grid-template-columns:1fr}}.fields{{padding:0 18px 20px}}.note{{padding:0 18px 20px}}.stats div{{border-right:0;border-bottom:1px solid var(--line)}}.stats div:last-child{{border-bottom:0}}.actions{{padding:20px 18px}}.remote-follow input{{width:min(100%,260px)}}article a{{padding:16px}}.posts-header{{align-items:flex-start;flex-direction:column}}}}
</style>
</head>
<body>
<main>
<section class="shell">
<div class="cover" aria-hidden="true"></div>
<div class="profile">{avatar_html}<div class="identity"><h1>{display_name}</h1><p class="handle">{username}</p><div class="badges">{badges_html}</div><div class="profile-actions"><form class="remote-follow" action="{profile_url}/remote-follow" method="get"><input name="domain" inputmode="url" autocomplete="url" placeholder="your.server or @you@server" aria-label="Your home server domain or handle" required><button class="button primary" type="submit">Remote follow</button></form></div></div></div>
<div class="note">{bio_html}</div>
{fields_html}
<div class="stats"><div><span class="num">{statuses}</span><span class="label">Posts</span></div><div><span class="num">{followers}</span><span class="label">Followers</span></div><div><span class="num">{following}</span><span class="label">Following</span></div></div>
<div class="actions"><a class="button" href="{profile_url}/statuses">Public posts</a></div>
</section>
{posts_section}
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

fn profile_display_name_source(account: &LocalAccount) -> String {
    if account.display_name().trim().is_empty() {
        format!("@{}", account.username())
    } else {
        account.display_name().to_owned()
    }
}

fn profile_header_style(header_url: Option<&str>) -> String {
    header_url
        .map(|url| format!("background-image:url('{}')", css_single_quoted_value(url)))
        .unwrap_or_default()
}

fn profile_avatar_html(
    display_name_source: &str,
    escaped_display_name: &str,
    avatar_url: Option<&str>,
) -> String {
    avatar_url
        .map(|url| {
            format!(
                "<img class=\"avatar\" src=\"{}\" alt=\"{}\" loading=\"lazy\">",
                escape_html(url),
                escaped_display_name
            )
        })
        .unwrap_or_else(|| {
            format!(
                "<div class=\"avatar avatar-fallback\">{}</div>",
                profile_initial(display_name_source)
            )
        })
}

fn profile_bio_html(account: &LocalAccount) -> String {
    if account.bio_html().trim().is_empty() {
        "<p class=\"muted\">No profile note yet.</p>".to_owned()
    } else {
        account.bio_html().to_owned()
    }
}

fn profile_fields_html(account: &LocalAccount) -> String {
    if account.fields().is_empty() {
        return String::new();
    }

    format!(
        "<dl class=\"fields\">{}</dl>",
        account
            .fields()
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
}

fn profile_badges_html(account: &LocalAccount) -> String {
    [
        account
            .is_locked()
            .then_some("<span class=\"badge\">Locked</span>"),
        account
            .is_bot()
            .then_some("<span class=\"badge\">Bot</span>"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("")
}

fn profile_posts_section(profile_url: &str, posts_html: &str) -> String {
    if posts_html.is_empty() {
        return String::new();
    }

    format!(
        r#"<section class="posts"><div class="posts-header"><h2>Recent posts</h2><a href="{profile_url}/statuses">Public posts</a></div><div class="feed">{posts_html}</div></section>"#
    )
}

fn profile_html_response(html: String) -> Result<Response> {
    let mut response = Response::from_body(ResponseBody::Body(html.into_bytes()))?;
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    response.headers_mut().set("Vary", "Accept")?;
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

    cache_public_response(
        Response::from_json(&build_tag_response(&db, &config, &tag).await?)?,
        CACHE_TTL_TRENDS,
    )
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
    let mut ordered_items = list_follower_actor_uris(&db, account.id()).await?;
    let mut seen = ordered_items.iter().cloned().collect::<HashSet<_>>();
    for username in list_local_follower_usernames(&db, account.id()).await? {
        let actor_uri = actor_url(&config, &username);
        if seen.insert(actor_uri.clone()) {
            ordered_items.push(actor_uri);
        }
    }
    let collection_id = format!("{}/followers", actor_url(&config, account.username()));
    let cache_tag = format!("account-{username}");
    cache_public_json_response(
        &build_ordered_collection_document(&collection_id, &ordered_items, &query),
        "application/activity+json",
        CACHE_TTL_FEDERATION,
        &[("Cache-Tag", &cache_tag)],
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
    let ordered_items = list_following_actor_uris(&db, account.id()).await?;
    let collection_id = format!("{}/following", actor_url(&config, account.username()));

    let cache_tag = format!("account-{username}");
    cache_public_json_response(
        &build_ordered_collection_document(&collection_id, &ordered_items, &query),
        "application/activity+json",
        CACHE_TTL_FEDERATION,
        &[("Cache-Tag", &cache_tag)],
    )
}

pub(crate) async fn outbox_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
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

    let actor = actor_url(&config, account.username());
    let outbox = format!("{actor}/outbox");
    let total_items = count_public_outbox_statuses(&db, account.id()).await?;
    let limit = query.limit.unwrap_or(20).clamp(1, 80);
    let offset = query.offset.unwrap_or(0);
    let cache_tag = format!("account-{username}");

    if query.page.unwrap_or(false) || query.offset.unwrap_or(0) > 0 {
        let statuses = list_public_outbox_statuses_page(&db, account.id(), limit, offset).await?;
        let ordered_items = build_outbox_activities(&db, &config, &account, &statuses).await?;
        let next_offset = offset.saturating_add(ordered_items.len() as u32);
        let next = if (next_offset as u64) < total_items {
            Some(format!(
                "{outbox}?page=true&offset={next_offset}&limit={limit}"
            ))
        } else {
            None
        };

        return cache_public_json_response(
            &serde_json::json!({
                "@context": "https://www.w3.org/ns/activitystreams",
                "type": "OrderedCollectionPage",
                "id": format!("{outbox}?page=true&offset={offset}&limit={limit}"),
                "partOf": outbox,
                "next": next,
                "orderedItems": ordered_items,
            }),
            "application/activity+json",
            CACHE_TTL_FEDERATION,
            &[("Cache-Tag", &cache_tag)],
        );
    }

    cache_public_json_response(
        &serde_json::json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "type": "OrderedCollection",
            "id": outbox,
            "totalItems": total_items,
            "first": format!("{outbox}?page=true&offset=0&limit={limit}"),
        }),
        "application/activity+json",
        CACHE_TTL_FEDERATION,
        &[("Cache-Tag", &cache_tag)],
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

#[cfg(test)]
mod tests {
    use super::{
        WebFingerLink, WebFingerResponse, escape_xml_attr, filter_webfinger_links,
        parse_webfinger_query_pairs, profile_avatar_html, profile_header_style,
        profile_posts_section,
    };

    #[test]
    fn profile_avatar_html_uses_escaped_image_when_avatar_is_configured() {
        let html = profile_avatar_html(
            "Alice",
            "Alice &amp; Bob",
            Some("https://media.example/avatar?a=1&b=2"),
        );

        assert!(html.contains("src=\"https://media.example/avatar?a=1&amp;b=2\""));
        assert!(html.contains("alt=\"Alice &amp; Bob\""));
        assert!(!html.contains("avatar-fallback"));
    }

    #[test]
    fn profile_avatar_html_falls_back_to_initial() {
        let html = profile_avatar_html("@alice", "@alice", None);

        assert_eq!(html, "<div class=\"avatar avatar-fallback\">a</div>");
    }

    #[test]
    fn profile_header_style_escapes_single_quoted_css_value() {
        let style = profile_header_style(Some("https://media.example/headers/alice's.png"));

        assert_eq!(
            style,
            "background-image:url('https://media.example/headers/alice\\'s.png')"
        );
    }

    #[test]
    fn profile_posts_section_omits_empty_feed() {
        assert_eq!(
            profile_posts_section("https://social.example/@alice", ""),
            ""
        );

        let html =
            profile_posts_section("https://social.example/@alice", "<article>hello</article>");
        assert!(html.contains("Recent posts"));
        assert!(html.contains("https://social.example/@alice/statuses"));
        assert!(html.contains("<article>hello</article>"));
    }

    #[test]
    fn webfinger_document_matches_mastodon_shape() {
        let config = cfwdon_core::AppConfig::new("example.com", "cfwdon", "test instance");
        let username = "alice";
        let actor = crate::actor_url(&config, username);
        let profile = crate::account_profile_page_url(&config, username);
        let document = WebFingerResponse {
            subject: format!("acct:{username}@{}", crate::instance_host(&config)),
            aliases: vec![profile.clone(), actor.clone()],
            links: vec![
                WebFingerLink::profile_page_link(profile),
                WebFingerLink::self_link(actor),
                WebFingerLink::subscribe_link(crate::authorize_interaction_subscribe_template(
                    &config,
                )),
                WebFingerLink::create_intent_link(crate::share_create_template(&config)),
                WebFingerLink::object_intent_link(crate::authorize_interaction_object_template(
                    &config,
                )),
            ],
        };
        let value = serde_json::to_value(document).unwrap();
        assert_eq!(
            value["links"][0]["rel"],
            "http://webfinger.net/rel/profile-page"
        );
        assert_eq!(value["links"][1]["rel"], "self");
        assert_eq!(
            value["links"][2]["template"],
            "https://example.com/authorize_interaction?uri={uri}"
        );
        assert_eq!(value["links"][3]["rel"], "https://w3id.org/fep/3b86/Create");
        assert_eq!(
            value["links"][3]["template"],
            "https://example.com/share?text={content}"
        );
        assert_eq!(value["links"][4]["rel"], "https://w3id.org/fep/3b86/Object");
        assert!(
            value["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|alias| alias.as_str() == Some("https://example.com/@alice"))
        );
    }

    #[test]
    fn host_meta_body_includes_webfinger_lrdd_template() {
        let config = cfwdon_core::AppConfig::new("example.com", "cfwdon", "test instance");
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <XRD xmlns=\"http://docs.oasis-open.org/ns/xri/xrd-1.0\">\n\
               <Link rel=\"lrdd\" template=\"{}\"/>\n\
             </XRD>\n",
            escape_xml_attr(&crate::webfinger_lrdd_template(&config))
        );
        assert!(
            body.contains("template=\"https://example.com/.well-known/webfinger?resource={uri}\"")
        );
    }

    #[test]
    fn parse_webfinger_query_pairs_requires_resource() {
        let error = parse_webfinger_query_pairs(Vec::<(&str, &str)>::new()).unwrap_err();
        assert!(error.contains("resource"));

        let error = parse_webfinger_query_pairs([("resource", "   ")]).unwrap_err();
        assert!(error.contains("resource"));
    }

    #[test]
    fn parse_webfinger_query_pairs_collects_multiple_rels() {
        let query = parse_webfinger_query_pairs([
            ("resource", "acct:alice@example.com"),
            ("rel", "self"),
            ("rel", "http://webfinger.net/rel/profile-page"),
            ("rel", ""),
        ])
        .unwrap();

        assert_eq!(query.resource, "acct:alice@example.com");
        assert_eq!(
            query.rels,
            vec![
                "self".to_owned(),
                "http://webfinger.net/rel/profile-page".to_owned()
            ]
        );
    }

    #[test]
    fn filter_webfinger_links_keeps_all_when_rel_absent() {
        let filtered = filter_webfinger_links(
            vec![
                WebFingerLink::self_link("https://example.com/users/alice".to_owned()),
                WebFingerLink::profile_page_link("https://example.com/@alice".to_owned()),
            ],
            &[],
        );
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].rel, "self");
        assert_eq!(filtered[1].rel, "http://webfinger.net/rel/profile-page");
    }

    #[test]
    fn filter_webfinger_links_selects_requested_rels() {
        let links = vec![
            WebFingerLink::self_link("https://example.com/users/alice".to_owned()),
            WebFingerLink::profile_page_link("https://example.com/@alice".to_owned()),
            WebFingerLink::subscribe_link(
                "https://example.com/authorize_interaction?uri={uri}".to_owned(),
            ),
        ];
        let filtered = filter_webfinger_links(
            links,
            &[
                "self".to_owned(),
                "http://webfinger.net/rel/avatar".to_owned(),
            ],
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].rel, "self");
    }

    #[test]
    fn filter_webfinger_links_empty_when_no_match() {
        let links = vec![WebFingerLink::self_link(
            "https://example.com/users/alice".to_owned(),
        )];
        let filtered =
            filter_webfinger_links(links, &["http://webfinger.net/rel/avatar".to_owned()]);
        assert!(filtered.is_empty());
    }
}
