use crate::{
    AppConfig, LocalAccount, MediaAttachmentRow, MediaKind, RemoteActorRow,
    RemoteStatusAttachmentRow, RemoteStatusRow, Response, Result, StatusRow, classify_media_kind,
    escape_html, local_status_ap_id, media_attachment_url, strip_html_tags,
};
use worker::ResponseBody;

pub(crate) fn account_statuses_html_response(
    config: &AppConfig,
    display_name: &str,
    handle: &str,
    profile_url: &str,
    statuses: &[String],
    older_page_url: Option<&str>,
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
    let pagination_html = older_page_url
        .map(|url| {
            format!(
                "<nav class=\"pager\"><a class=\"button\" rel=\"next\" href=\"{}\">Older posts</a></nav>",
                escape_html(url)
            )
        })
        .unwrap_or_default();
    let infinite_scroll_script = older_page_url
        .map(|_| ACCOUNT_STATUSES_INFINITE_SCROLL_SCRIPT)
        .unwrap_or_default();
    let html = format!(
        r#"<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>
:root{{color-scheme:dark;--bg:#101114;--panel:#181b20;--line:#30343c;--text:#f4f0e8;--muted:#a9adb7;--accent:#45c08d;--accent-2:#f2b84b;--ink:#0f1411}}
*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;background:linear-gradient(135deg,#101114 0%,#171a1f 58%,#1f241e 100%);color:var(--text);font-family:Inter,ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;line-height:1.55}}a{{color:inherit}}main{{width:min(820px,100%);margin:0 auto;padding:32px 20px 56px}}header{{display:flex;align-items:flex-end;justify-content:space-between;gap:20px;margin-bottom:24px}}h1{{margin:0;font-size:clamp(34px,6vw,56px);line-height:1.03;letter-spacing:0}}.handle,.meta,.empty,time{{color:var(--muted)}}.nav,.pager{{display:flex;gap:10px;flex-wrap:wrap}}.button{{display:inline-flex;align-items:center;justify-content:center;min-height:42px;padding:0 16px;border:1px solid var(--line);border-radius:8px;text-decoration:none;font-weight:650}}.primary{{background:var(--accent);border-color:var(--accent);color:var(--ink)}}.feed{{display:grid;gap:14px}}article{{border:1px solid var(--line);border-radius:8px;background:rgba(24,27,32,.92);padding:20px;box-shadow:0 18px 54px rgba(0,0,0,.22)}}article>a{{display:block;text-decoration:none}}.content{{font-size:18px;overflow-wrap:anywhere}}.content p:first-child{{margin-top:0}}.content p:last-child{{margin-bottom:0}}.spoiler{{margin:0 0 12px;color:var(--accent-2);font-weight:700}}.media{{display:grid;gap:10px;margin-top:14px}}.media img{{display:block;width:100%;max-height:520px;object-fit:contain;border-radius:8px;background:#0d0f13}}time{{display:block;margin-top:16px;font-size:13px}}.pager{{justify-content:center;margin-top:18px}}.pager[aria-busy="true"] .button{{opacity:.72;pointer-events:none}}.empty{{border:1px solid var(--line);border-radius:8px;padding:24px;background:rgba(24,27,32,.92)}}footer{{margin-top:20px;color:var(--muted);font-size:13px;text-align:center}}@media (max-width:640px){{main{{padding:18px 12px 42px}}header{{display:block}}.nav{{margin-top:18px}}article{{padding:16px}}.media img{{max-height:360px}}}}
</style>
</head>
<body>
<main>
<header><div><p class="meta">{instance_name}</p><h1>{name}</h1><p class="handle">{handle}</p></div><nav class="nav"><a class="button primary" href="{profile_url}">Profile</a></nav></header>
<section class="feed">{statuses_html}</section>
{pagination_html}
<footer>Public posts</footer>
</main>
{infinite_scroll_script}
</body>
</html>"#
    );
    let mut response = Response::from_body(ResponseBody::Body(html.into_bytes()))?;
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    Ok(response)
}

const ACCOUNT_STATUSES_INFINITE_SCROLL_SCRIPT: &str = r#"<script>
(() => {
  const feed = document.querySelector(".feed");
  const pager = document.querySelector(".pager");
  if (!feed || !pager || !("IntersectionObserver" in window) || !window.DOMParser) {
    return;
  }

  let loading = false;
  const nextLink = () => pager.querySelector('a[rel="next"]');
  const observer = new IntersectionObserver(async (entries) => {
    if (loading || !entries.some((entry) => entry.isIntersecting)) {
      return;
    }
    const link = nextLink();
    if (!link) {
      observer.disconnect();
      return;
    }

    loading = true;
    pager.setAttribute("aria-busy", "true");
    const originalText = link.textContent;
    link.textContent = "Loading...";
    try {
      const response = await fetch(link.href, {
        headers: { Accept: "text/html" },
        credentials: "same-origin"
      });
      if (!response.ok) {
        throw new Error(`Failed to load ${response.status}`);
      }
      const documentNext = new DOMParser().parseFromString(await response.text(), "text/html");
      const articles = documentNext.querySelectorAll(".feed article");
      for (const article of articles) {
        feed.appendChild(document.importNode(article, true));
      }
      const next = documentNext.querySelector('.pager a[rel="next"]');
      if (next) {
        link.href = next.href;
        link.textContent = originalText || "Older posts";
      } else {
        observer.disconnect();
        pager.remove();
      }
    } catch (_error) {
      link.textContent = originalText || "Older posts";
      observer.disconnect();
    } finally {
      loading = false;
      pager.removeAttribute("aria-busy");
    }
  }, { rootMargin: "700px 0px" });

  observer.observe(pager);
})();
</script>"#;

pub(crate) fn local_status_html_item(
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
    media: &[MediaAttachmentRow],
) -> String {
    let status_url = escape_html(&local_status_ap_id(config, account, status));
    let media_html = local_media_html(config, media);
    status_html_item(
        &status_url,
        &status.content_html,
        &status.spoiler_text,
        &status.created_at,
        &media_html,
    )
}

pub(crate) fn remote_status_html_item(
    actor: &RemoteActorRow,
    status: &RemoteStatusRow,
    media: &[RemoteStatusAttachmentRow],
) -> String {
    let status_url = escape_html(status.url.as_deref().unwrap_or(status.object_uri.as_str()));
    let _actor = actor;
    let media_html = remote_media_html(media);
    status_html_item(
        &status_url,
        &status.content_html,
        &status.spoiler_text,
        &status.published_at,
        &media_html,
    )
}

fn status_html_item(
    url: &str,
    content_html: &str,
    spoiler_text: &str,
    published_at: &str,
    media_html: &str,
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
        "<article><a href=\"{url}\" aria-label=\"{}\"><div class=\"content\">{}{}</div>{media_html}<time>{}</time></a></article>",
        escape_html(&aria_label),
        spoiler,
        content_html,
        escape_html(published_at)
    )
}

fn local_media_html(config: &AppConfig, media: &[MediaAttachmentRow]) -> String {
    let images = media
        .iter()
        .filter(|attachment| {
            classify_media_kind(&attachment.content_type) == Some(MediaKind::Image)
        })
        .map(|attachment| {
            (
                media_attachment_url(config, &attachment.id, &attachment.object_key),
                attachment.description.clone(),
            )
        })
        .collect::<Vec<_>>();
    media_html(images)
}

fn remote_media_html(media: &[RemoteStatusAttachmentRow]) -> String {
    let images = media
        .iter()
        .filter(|attachment| {
            classify_media_kind(&attachment.content_type) == Some(MediaKind::Image)
        })
        .map(|attachment| {
            (
                attachment
                    .preview_url
                    .clone()
                    .unwrap_or_else(|| attachment.remote_url.clone()),
                attachment.description.clone().unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    media_html(images)
}

fn media_html(images: Vec<(String, String)>) -> String {
    let images = images
        .into_iter()
        .map(|(url, description)| {
            format!(
                "<img src=\"{}\" alt=\"{}\" loading=\"lazy\">",
                escape_html(&url),
                escape_html(&description)
            )
        })
        .collect::<Vec<_>>();
    if images.is_empty() {
        String::new()
    } else {
        format!("<div class=\"media\">{}</div>", images.join(""))
    }
}
