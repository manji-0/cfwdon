use crate::{
    CACHE_TTL_STATIC_METADATA, Request, Response, Result, RouteContext, account_profile_page_url,
    actor_url, authorize_interaction_object_template, authorize_interaction_subscribe_template,
    cache_public_json_response, find_account_by_username, instance_host, load_config,
    media_object_url, parse_webfinger_resource, share_create_template,
};

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

    let db = crate::bind_request_d1(&ctx, &config)?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
