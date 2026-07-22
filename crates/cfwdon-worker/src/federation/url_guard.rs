use cfwdon_domain::{
    RemoteDnsResolutionIssue, RemoteUrlPolicyIssue, parse_dns_answer_ips,
    remote_hostname_dns_resolution_allowed, remote_url_policy_for_ip, remote_url_policy_from_parts,
};
use serde::Deserialize;
use std::net::IpAddr;
use url::{Host, Url};
use worker::{Error, Fetch, Headers, Method, Request, RequestInit, Result};
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

/// Validate remote fetch URL policy and DNS answers via DoH.
///
/// Cloudflare Workers `Fetch` resolves hostnames independently of this check, so
/// this is best-effort SSRF filtering rather than true IP pinning. Redirect
/// targets are re-validated before each hop.
pub(crate) async fn validate_remote_fetch_url(url: &Url) -> Result<()> {
    let has_userinfo = !url.username().is_empty() || url.password().is_some();
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::RustError("unsupported remote URL scheme".to_owned()));
    }

    match url.host() {
        None => Err(Error::RustError("remote URL must include host".to_owned())),
        Some(Host::Domain(host)) => {
            if let Err(issue) = remote_url_policy_from_parts(url.scheme(), host, has_userinfo) {
                return Err(remote_url_policy_error(issue));
            }
            validate_remote_hostname_resolution(host).await
        }
        Some(Host::Ipv4(v4)) => {
            remote_url_policy_for_ip(IpAddr::V4(v4), has_userinfo).map_err(remote_url_policy_error)
        }
        Some(Host::Ipv6(v6)) => {
            remote_url_policy_for_ip(IpAddr::V6(v6), has_userinfo).map_err(remote_url_policy_error)
        }
    }
}

fn remote_url_policy_error(issue: RemoteUrlPolicyIssue) -> Error {
    Error::RustError(match issue {
        RemoteUrlPolicyIssue::UnsupportedScheme => "unsupported remote URL scheme".to_owned(),
        RemoteUrlPolicyIssue::MissingHost => "remote URL must include host".to_owned(),
        RemoteUrlPolicyIssue::UserInfoPresent => "remote URL must not include user info".to_owned(),
        RemoteUrlPolicyIssue::LocalhostBlocked => "localhost is not allowed".to_owned(),
        RemoteUrlPolicyIssue::BlockedIp => "private or loopback IPs are not allowed".to_owned(),
    })
}

async fn validate_remote_hostname_resolution(host: &str) -> Result<()> {
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

    Ok(())
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
