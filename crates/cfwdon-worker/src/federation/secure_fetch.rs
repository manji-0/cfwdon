use std::collections::HashSet;
use std::net::IpAddr;
use worker::{Error, Fetch, Headers, Method, Request, RequestInit, RequestRedirect, Result};

use super::{parse_remote_http_url, validate_remote_fetch_url};

const MAX_REMOTE_FETCH_REDIRECTS: usize = 5;

pub(crate) async fn fetch_remote_http_json(url: &str, accept: &str) -> Result<serde_json::Value> {
    let mut current_url = parse_remote_http_url(url)?;
    validate_remote_fetch_url(&current_url).await?;

    for redirect_count in 0..=MAX_REMOTE_FETCH_REDIRECTS {
        let headers = Headers::new();
        headers.set("Accept", accept)?;
        let mut init = RequestInit::new();
        init.with_method(Method::Get)
            .with_headers(headers)
            .with_redirect(RequestRedirect::Manual);
        let request = Request::new_with_init(current_url.as_str(), &init)?;
        let mut response = Fetch::Request(request).send().await?;
        let status = response.status_code();

        if (300..400).contains(&status) {
            if redirect_count == MAX_REMOTE_FETCH_REDIRECTS {
                return Err(Error::RustError(format!(
                    "remote fetch exceeded redirect limit for {url}"
                )));
            }
            let location = response.headers().get("Location")?.ok_or_else(|| {
                Error::RustError(format!(
                    "remote fetch redirect missing Location header for {}",
                    current_url
                ))
            })?;
            current_url = parse_remote_http_url(&location)?;
            validate_remote_fetch_url(&current_url).await?;
            continue;
        }

        if status / 100 != 2 {
            return Err(Error::RustError(format!(
                "failed to fetch remote document {}: HTTP {}",
                current_url, status
            )));
        }

        return response.json().await;
    }

    Err(Error::RustError(format!(
        "remote fetch exceeded redirect limit for {url}"
    )))
}

pub(crate) async fn validate_remote_actor_profile_url(
    raw_url: &str,
    validated_ip_hosts: &mut HashSet<IpAddr>,
) -> Result<()> {
    let parsed = parse_remote_http_url(raw_url)?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(Error::RustError(
            "remote URL must not include user info".to_owned(),
        ));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::RustError("remote URL must include host".to_owned()))?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return Err(Error::RustError("localhost is not allowed".to_owned()));
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if validated_ip_hosts.insert(ip) {
            validate_remote_fetch_url(&parsed).await?;
        }
        return Ok(());
    }
    validate_remote_fetch_url(&parsed).await
}
