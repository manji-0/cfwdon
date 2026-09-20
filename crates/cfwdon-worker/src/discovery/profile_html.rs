use crate::{
    AccountStats, AppConfig, LocalAccount, Response, Result, activitypub_datetime_string,
    actor_url, escape_html, instance_host, media_object_url, render_profile_field_value_html,
};
use worker::ResponseBody;

pub(in crate::discovery) fn profile_html_document(
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

pub(in crate::discovery) fn profile_html_response(html: String) -> Result<Response> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
