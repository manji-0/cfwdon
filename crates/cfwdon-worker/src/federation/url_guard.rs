use cfwdon_domain::{
    RemoteDnsResolutionIssue, RemoteUrlPolicyIssue, host_is_ip_literal, parse_dns_answer_ips,
    remote_hostname_dns_resolution_allowed, remote_url_policy_from_parts,
};
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::IpAddr;
use url::Url;
use worker::{Error, Fetch, Headers, Method, Request, RequestInit, Result};

const REMOTE_HOST_VALIDATION_CACHE_TTL_MS: f64 = 5.0 * 60.0 * 1000.0;

thread_local! {
    static REMOTE_HOST_VALIDATION_CACHE: RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());
}

#[derive(Debug, Deserialize)]
pub(crate) struct DnsJsonResponse {
    #[serde(rename = "Status")]
    pub(crate) status: u32,
    #[serde(rename = "Answer")]
    pub(crate) answer: Option<Vec<DnsJsonAnswer>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DnsJsonAnswer {
    pub(crate) data: String,
}

pub(crate) async fn validate_remote_fetch_url(url: &Url) -> Result<()> {
    let host = url
        .host_str()
        .ok_or_else(|| Error::RustError("remote URL must include host".to_owned()))?;
    if let Err(issue) = remote_url_policy_from_parts(
        url.scheme(),
        host,
        !url.username().is_empty() || url.password().is_some(),
    ) {
        return Err(Error::RustError(match issue {
            RemoteUrlPolicyIssue::UnsupportedScheme => "unsupported remote URL scheme".to_owned(),
            RemoteUrlPolicyIssue::MissingHost => "remote URL must include host".to_owned(),
            RemoteUrlPolicyIssue::UserInfoPresent => {
                "remote URL must not include user info".to_owned()
            }
            RemoteUrlPolicyIssue::LocalhostBlocked => "localhost is not allowed".to_owned(),
            RemoteUrlPolicyIssue::BlockedIp => "private or loopback IPs are not allowed".to_owned(),
        }));
    }
    if !host_is_ip_literal(host) {
        validate_remote_hostname_resolution(host).await?;
    }

    Ok(())
}

async fn validate_remote_hostname_resolution(host: &str) -> Result<()> {
    if remote_hostname_validation_cache_hit(host) {
        return Ok(());
    }

    let mut resolved = Vec::new();
    resolved.extend(resolve_dns_json_ips(host, "A").await?);
    resolved.extend(resolve_dns_json_ips(host, "AAAA").await?);
    if let Err(issue) = remote_hostname_dns_resolution_allowed(&resolved) {
        return Err(Error::RustError(match issue {
            RemoteDnsResolutionIssue::NoRecords => {
                format!("remote host {host} did not resolve to any public A/AAAA records")
            }
            RemoteDnsResolutionIssue::BlockedAddress => {
                format!("remote host {host} resolved to a blocked IP range")
            }
        }));
    }

    cache_remote_hostname_validation(host);
    Ok(())
}

fn remote_hostname_validation_cache_hit(host: &str) -> bool {
    let now_ms = js_sys::Date::now();
    REMOTE_HOST_VALIDATION_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.retain(|_, expires_at_ms| *expires_at_ms > now_ms);
        cache
            .get(host)
            .is_some_and(|expires_at_ms| *expires_at_ms > now_ms)
    })
}

fn cache_remote_hostname_validation(host: &str) {
    let expires_at_ms = js_sys::Date::now() + REMOTE_HOST_VALIDATION_CACHE_TTL_MS;
    REMOTE_HOST_VALIDATION_CACHE.with(|cache| {
        cache.borrow_mut().insert(host.to_owned(), expires_at_ms);
    });
}

async fn resolve_dns_json_ips(host: &str, record_type: &str) -> Result<Vec<IpAddr>> {
    let headers = Headers::new();
    headers.set("Accept", "application/dns-json")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);
    let encoded_host = url::form_urlencoded::byte_serialize(host.as_bytes()).collect::<String>();
    let url =
        format!("https://cloudflare-dns.com/dns-query?name={encoded_host}&type={record_type}");
    let request = Request::new_with_init(&url, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    if response.status_code() / 100 != 2 {
        return Err(Error::RustError(format!(
            "DNS resolution failed for {host}: HTTP {}",
            response.status_code()
        )));
    }

    let body: DnsJsonResponse = response.json().await?;
    if body.status != 0 {
        return Err(Error::RustError(format!(
            "DNS resolution failed for {host}: response status {}",
            body.status
        )));
    }

    Ok(parse_dns_answer_ips(
        body.answer
            .unwrap_or_default()
            .iter()
            .map(|answer| answer.data.as_str()),
    ))
}
