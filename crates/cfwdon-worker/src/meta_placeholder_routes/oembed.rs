use crate::activitypub::{is_public_activitypub_visibility, local_status_ap_id};
use crate::auth::find_account_by_id;
use crate::db_session::bind_request_d1;
use crate::identity::{actor_url, instance_base_url};
use crate::response_utils::{CACHE_TTL_OEMBED, cache_public_response};
use crate::runtime_config::load_config;
use crate::statuses::find_local_status_by_object_uri;
use crate::time_html::escape_html;
use serde::Deserialize;
use worker::{Request, Response, Result, RouteContext};

#[derive(Debug, Deserialize)]
struct OembedQuery {
    url: String,
    maxwidth: Option<u32>,
    maxheight: Option<u32>,
    format: Option<String>,
}

fn build_oembed_html(
    account: &cfwdon_domain::LocalAccount,
    status_url: &str,
    content_html: &str,
) -> String {
    format!(
        concat!(
            "<blockquote class=\"mastodon-embed\" ",
            "style=\"background:#FCF8FF;border-radius:8px;border:1px solid #C9C4DA;",
            "margin:0;max-width:540px;min-width:270px;overflow:hidden;padding:24px;\">",
            "<div style=\"color:#1C1A25;font-family:system-ui,-apple-system,BlinkMacSystemFont,",
            "'Segoe UI',Oxygen,Ubuntu,Cantarell,'Fira Sans','Droid Sans','Helvetica Neue',Roboto,sans-serif;",
            "font-size:14px;letter-spacing:0.25px;line-height:20px;\">",
            "{content_html}",
            "</div>",
            "<div style=\"color:#787588;font-family:system-ui,-apple-system,BlinkMacSystemFont,",
            "'Segoe UI',Oxygen,Ubuntu,Cantarell,'Fira Sans','Droid Sans','Helvetica Neue',Roboto,sans-serif;",
            "font-size:14px;letter-spacing:0.25px;line-height:20px;margin-top:16px;\">",
            "Post by @{username}",
            "</div>",
            "<a href=\"{status_url}\" ",
            "style=\"align-items:center;color:#1C1A25;display:flex;flex-direction:column;",
            "font-family:system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',Oxygen,Ubuntu,",
            "Cantarell,'Fira Sans','Droid Sans','Helvetica Neue',Roboto,sans-serif;font-size:14px;",
            "font-weight:500;justify-content:center;letter-spacing:0.25px;line-height:20px;",
            "margin-top:16px;text-decoration:none;\">",
            "View on cfwdon",
            "</a>",
            "</blockquote>"
        ),
        content_html = content_html,
        username = escape_html(account.username()),
        status_url = escape_html(status_url),
    )
}

const OEMBED_DEFAULT_WIDTH: u32 = 400;
const OEMBED_DEFAULT_HEIGHT: u32 = 200;

fn oembed_capped_dimension(default: u32, requested_max: Option<u32>) -> u32 {
    match requested_max {
        Some(max) => default.min(max),
        None => default,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OembedFormatDecision {
    Json,
    UnsupportedXml,
    Unrecognized,
}

fn resolve_oembed_format(format: Option<&str>) -> OembedFormatDecision {
    match format.map(|value| value.trim().to_ascii_lowercase()) {
        None => OembedFormatDecision::Json,
        Some(ref value) if value.is_empty() || value == "json" => OembedFormatDecision::Json,
        Some(ref value) if value == "xml" => OembedFormatDecision::UnsupportedXml,
        Some(_) => OembedFormatDecision::Unrecognized,
    }
}

fn build_oembed_document(
    config: &cfwdon_core::AppConfig,
    account: &cfwdon_domain::LocalAccount,
    status_url: &str,
    content_html: &str,
    author_name: &str,
    maxwidth: Option<u32>,
    maxheight: Option<u32>,
) -> serde_json::Value {
    serde_json::json!({
        "type": "rich",
        "version": "1.0",
        "title": format!("New status by {}", account.username()),
        "author_name": author_name,
        "author_url": actor_url(config, account.username()),
        "provider_name": config.instance_domain,
        "provider_url": format!("{}/", instance_base_url(config)),
        "cache_age": 86400,
        "html": build_oembed_html(account, status_url, content_html),
        "width": oembed_capped_dimension(OEMBED_DEFAULT_WIDTH, maxwidth),
        "height": oembed_capped_dimension(OEMBED_DEFAULT_HEIGHT, maxheight),
    })
}

pub(crate) async fn oembed_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: OembedQuery = req.query()?;
    match resolve_oembed_format(query.format.as_deref()) {
        OembedFormatDecision::Json => {}
        OembedFormatDecision::UnsupportedXml => {
            return Response::error("Not Implemented", 501);
        }
        OembedFormatDecision::Unrecognized => {
            return Response::error("Bad Request", 400);
        }
    }
    let db = bind_request_d1(&ctx, &config)?;
    let Some(status) = find_local_status_by_object_uri(&db, &config, &query.url).await? else {
        return Response::error("Record not found", 404);
    };
    if !is_public_activitypub_visibility(status.visibility.as_str()) {
        return Response::error("Record not found", 404);
    }
    let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
        return Response::error("Record not found", 404);
    };

    let status_url = local_status_ap_id(&config, &account, &status);
    let author_name = if account.display_name().trim().is_empty() {
        account.username().to_owned()
    } else {
        account.display_name().to_owned()
    };

    cache_public_response(
        Response::from_json(&build_oembed_document(
            &config,
            &account,
            &status_url,
            &status.content_html,
            &author_name,
            query.maxwidth,
            query.maxheight,
        ))?,
        CACHE_TTL_OEMBED,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oembed_height_defaults_to_integer_when_maxheight_absent() {
        let config = cfwdon_core::AppConfig::new("https://social.example", "cfwdon", "test");
        let account = cfwdon_domain::LocalAccount::from_record(
            cfwdon_domain::LocalAccountRecord::test_fixture("acct-1", "alice"),
        );
        let document = build_oembed_document(
            &config,
            &account,
            "https://social.example/@alice/statuses/1",
            "<p>hi</p>",
            "alice",
            None,
            None,
        );
        assert_eq!(document["height"], serde_json::json!(OEMBED_DEFAULT_HEIGHT));
        assert!(document["height"].is_number());
        assert_eq!(document["width"], serde_json::json!(OEMBED_DEFAULT_WIDTH));
    }

    #[test]
    fn oembed_dimensions_respect_maxwidth_and_maxheight_caps() {
        assert_eq!(oembed_capped_dimension(400, Some(200)), 200);
        assert_eq!(oembed_capped_dimension(400, Some(800)), 400);
        assert_eq!(oembed_capped_dimension(200, Some(50)), 50);
        assert_eq!(oembed_capped_dimension(200, None), 200);
    }

    #[test]
    fn oembed_format_json_and_absent_are_accepted() {
        assert_eq!(resolve_oembed_format(None), OembedFormatDecision::Json);
        assert_eq!(
            resolve_oembed_format(Some("json")),
            OembedFormatDecision::Json
        );
        assert_eq!(
            resolve_oembed_format(Some("JSON")),
            OembedFormatDecision::Json
        );
    }

    #[test]
    fn oembed_format_xml_is_not_implemented() {
        assert_eq!(
            resolve_oembed_format(Some("xml")),
            OembedFormatDecision::UnsupportedXml
        );
    }

    #[test]
    fn oembed_format_unrecognized_is_bad_request() {
        assert_eq!(
            resolve_oembed_format(Some("yaml")),
            OembedFormatDecision::Unrecognized
        );
    }

    #[test]
    fn build_oembed_html_escapes_username_and_status_url() {
        let account = cfwdon_domain::LocalAccount::from_record(
            cfwdon_domain::LocalAccountRecord::test_fixture("acct-1", "alice\"onclick=x"),
        );
        let html = build_oembed_html(
            &account,
            "https://evil.example/\" onmouseover=\"alert(1)",
            "<p>safe already-escaped content</p>",
        );
        assert!(html.contains("Post by @alice&quot;onclick=x"));
        assert!(html.contains("href=\"https://evil.example/&quot; onmouseover=&quot;alert(1)\""));
        assert!(html.contains("<p>safe already-escaped content</p>"));
        assert!(!html.contains("Post by @alice\"onclick=x"));
    }
}
