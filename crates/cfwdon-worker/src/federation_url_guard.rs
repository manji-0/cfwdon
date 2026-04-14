use serde::Deserialize;
use std::net::IpAddr;
use url::Url;
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

pub(crate) async fn validate_remote_fetch_url(url: &Url) -> Result<()> {
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::RustError(
            "remote URL must not include user info".to_owned(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| Error::RustError("remote URL must include host".to_owned()))?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return Err(Error::RustError("localhost is not allowed".to_owned()));
    }
    if let Ok(ip) = host.parse::<IpAddr>()
        && is_blocked_ip_address(ip)
    {
        return Err(Error::RustError(
            "private or loopback IPs are not allowed".to_owned(),
        ));
    }
    if host.parse::<IpAddr>().is_err() {
        validate_remote_hostname_resolution(&host).await?;
    }

    Ok(())
}

async fn validate_remote_hostname_resolution(host: &str) -> Result<()> {
    let mut resolved = Vec::new();
    resolved.extend(resolve_dns_json_ips(host, "A").await?);
    resolved.extend(resolve_dns_json_ips(host, "AAAA").await?);
    if resolved.is_empty() {
        return Err(Error::RustError(format!(
            "remote host {host} did not resolve to any public A/AAAA records"
        )));
    }
    if resolved.iter().any(|ip| is_blocked_ip_address(*ip)) {
        return Err(Error::RustError(format!(
            "remote host {host} resolved to a blocked IP range"
        )));
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

    Ok(body
        .answer
        .unwrap_or_default()
        .into_iter()
        .filter_map(|answer| answer.data.parse::<IpAddr>().ok())
        .collect())
}

fn is_blocked_ip_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.is_broadcast()
                || v4.octets()[0] == 0
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
        }
    }
}
