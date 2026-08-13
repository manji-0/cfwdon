use std::collections::HashSet;
use std::net::IpAddr;
use url::Url;
use worker::{Error, Fetch, Headers, Method, Request, RequestInit, RequestRedirect, Result};

use super::{parse_remote_http_url, validate_remote_fetch_url};

const MAX_REMOTE_FETCH_REDIRECTS: usize = 5;

pub(crate) fn resolve_remote_redirect_location(base: &Url, location: &str) -> Result<Url> {
    let location = location.trim();
    if location.is_empty() {
        return Err(Error::RustError(
            "remote fetch redirect Location header is empty".to_owned(),
        ));
    }

    let resolved = if location.starts_with("http://") || location.starts_with("https://") {
        Url::parse(location).map_err(|error| {
            Error::RustError(format!("invalid remote redirect URL {location}: {error}"))
        })?
    } else {
        base.join(location).map_err(|error| {
            Error::RustError(format!(
                "failed to resolve remote redirect {location} against {}: {error}",
                base
            ))
        })?
    };

    match resolved.scheme() {
        "http" | "https" => Ok(resolved),
        scheme => Err(Error::RustError(format!(
            "unsupported remote redirect URL scheme {scheme}"
        ))),
    }
}

pub(crate) async fn fetch_remote_http_json(url: &str, accept: &str) -> Result<serde_json::Value> {
    let mut current_url = parse_remote_http_url(url)?;
    validate_remote_fetch_url(&current_url).await?;

    for redirect_count in 0..=MAX_REMOTE_FETCH_REDIRECTS {
        let headers = Headers::new();
        headers.set("Accept", accept)?;
        headers.set("User-Agent", "cfwdon (https://github.com/manji-0/cfwdon)")?;
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
            current_url = resolve_remote_redirect_location(&current_url, &location)?;
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
    match parsed.host() {
        None => Err(Error::RustError("remote URL must include host".to_owned())),
        Some(url::Host::Domain(host)) => {
            let host = host.trim_end_matches('.').to_ascii_lowercase();
            if host == "localhost" || host.ends_with(".localhost") {
                return Err(Error::RustError("localhost is not allowed".to_owned()));
            }
            validate_remote_fetch_url(&parsed).await
        }
        Some(url::Host::Ipv4(v4)) => {
            let ip = IpAddr::V4(v4);
            if validated_ip_hosts.insert(ip) {
                validate_remote_fetch_url(&parsed).await?;
            }
            Ok(())
        }
        Some(url::Host::Ipv6(v6)) => {
            let ip = IpAddr::V6(v6);
            if validated_ip_hosts.insert(ip) {
                validate_remote_fetch_url(&parsed).await?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    #[test]
    fn resolve_redirect_joins_relative_path() {
        let base = Url::parse("https://example.com/a/b").unwrap();
        let resolved = resolve_remote_redirect_location(&base, "/c").unwrap();
        assert_eq!(resolved.as_str(), "https://example.com/c");
    }

    #[test]
    fn resolve_redirect_joins_sibling_relative_path() {
        let base = Url::parse("https://example.com/a/b").unwrap();
        let resolved = resolve_remote_redirect_location(&base, "c").unwrap();
        assert_eq!(resolved.as_str(), "https://example.com/a/c");
    }

    #[test]
    fn resolve_redirect_accepts_absolute_https_url() {
        let base = Url::parse("https://example.com/a").unwrap();
        let resolved =
            resolve_remote_redirect_location(&base, "https://remote.example/object").unwrap();
        assert_eq!(resolved.as_str(), "https://remote.example/object");
    }

    #[test]
    fn resolve_redirect_rejects_empty_location() {
        let base = Url::parse("https://example.com/a").unwrap();
        assert!(resolve_remote_redirect_location(&base, "   ").is_err());
    }

    #[test]
    fn resolve_redirect_rejects_unsupported_scheme() {
        let base = Url::parse("https://example.com/a").unwrap();
        assert!(resolve_remote_redirect_location(&base, "ftp://example.com/x").is_err());
    }
}
