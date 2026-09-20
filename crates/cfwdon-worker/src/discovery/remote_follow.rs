use crate::{
    Error, Request, Response, Result, RouteContext, escape_html, find_account_by_username,
    instance_host, load_config, parse_webfinger_resource,
};
use url::Url;
use worker::ResponseBody;

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
    let domain = match parse_remote_follow_domain_query(req.url()?.query_pairs()) {
        Ok(domain) => domain,
        Err(message) => return Response::error(message, 400),
    };
    let remote_base = match remote_follow_base_url(&domain) {
        Ok(base) => base,
        Err(error) => return Response::error(error.to_string(), 400),
    };
    let db = crate::bind_request_d1(&ctx, &config)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let acct = format!("acct:{}@{}", account.username(), instance_host(&config));
    let location = format!(
        "{}/authorize_interaction?uri={}",
        remote_base.trim_end_matches('/'),
        urlencoding::encode(&acct)
    );
    redirect_response(&location)
}

/// Extract the required `domain` query parameter for remote-follow redirects.
///
/// Missing or blank values must be 400s; do not let serde `req.query()` map them
/// to handler `Err` → HTTP 500 via `router::handle_fetch`.
fn parse_remote_follow_domain_query<I, K, V>(pairs: I) -> std::result::Result<String, String>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut domain = None;
    for (key, value) in pairs {
        if key.as_ref() == "domain" {
            domain = Some(value.as_ref().to_owned());
        }
    }
    let domain = domain.unwrap_or_default();
    let domain = domain.trim();
    if domain.is_empty() {
        return Err("missing domain parameter".to_owned());
    }
    Ok(domain.to_owned())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_remote_follow_domain_query_requires_domain() {
        let error = parse_remote_follow_domain_query(Vec::<(&str, &str)>::new()).unwrap_err();
        assert!(error.contains("domain"));

        let error = parse_remote_follow_domain_query([("domain", "   ")]).unwrap_err();
        assert!(error.contains("domain"));

        assert_eq!(
            parse_remote_follow_domain_query([("domain", "Social.Example")]).unwrap(),
            "Social.Example"
        );
        assert_eq!(
            parse_remote_follow_domain_query([
                ("domain", "first.example"),
                ("domain", "second.example"),
            ])
            .unwrap(),
            "second.example"
        );
    }
}
