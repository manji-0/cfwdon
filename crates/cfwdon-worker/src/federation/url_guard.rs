use cfwdon_domain::{
    RemoteDnsResolutionIssue, RemoteUrlPolicyIssue, parse_dns_answer_ips,
    remote_hostname_dns_resolution_allowed, remote_url_policy_for_ip, remote_url_policy_from_parts,
};
use futures_util::future::join;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::IpAddr;
use url::{Host, Url};
use worker::kv::KvStore;
use worker::{Env, Error, Fetch, Headers, Method, Request, RequestInit, Result};

const DOH_CACHE_KEY_PREFIX: &str = "doh:v1:";
const DOH_CACHE_ALLOW_TTL_SECS: u64 = 300;
const DOH_CACHE_DENY_TTL_SECS: u64 = 60;
const DOH_L1_TTL_MS: f64 = 60_000.0;

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct DnsHostCacheEntry {
    allowed: bool,
}

thread_local! {
    static REMOTE_DNS_CACHE_KV: RefCell<Option<KvStore>> = const { RefCell::new(None) };
    static REMOTE_DNS_HOST_L1: RefCell<HashMap<String, (bool, f64)>> = RefCell::new(HashMap::new());
}

/// Install the optional KV binding used to cache remote hostname DoH policy results.
///
/// Missing bindings are ignored so local/unit paths keep working without KV.
pub(crate) fn install_remote_dns_cache(env: &Env, binding: &str) {
    let kv = env.kv(binding).ok();
    REMOTE_DNS_CACHE_KV.with(|slot| {
        *slot.borrow_mut() = kv;
    });
}

fn remote_dns_cache_kv() -> Option<KvStore> {
    REMOTE_DNS_CACHE_KV.with(|slot| slot.borrow().clone())
}

fn doh_cache_key(host: &str) -> String {
    format!("{DOH_CACHE_KEY_PREFIX}{}", host.to_ascii_lowercase())
}

fn l1_get(host: &str) -> Option<bool> {
    let now_ms = js_sys::Date::now();
    REMOTE_DNS_HOST_L1.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.retain(|_, (_, expires_at_ms)| *expires_at_ms > now_ms);
        cache
            .get(host)
            .filter(|(_, expires_at_ms)| *expires_at_ms > now_ms)
            .map(|(allowed, _)| *allowed)
    })
}

fn l1_put(host: &str, allowed: bool) {
    let expires_at_ms = js_sys::Date::now() + DOH_L1_TTL_MS;
    REMOTE_DNS_HOST_L1.with(|cache| {
        cache
            .borrow_mut()
            .insert(host.to_ascii_lowercase(), (allowed, expires_at_ms));
    });
}

async fn kv_get_allowed(host: &str) -> Option<bool> {
    let kv = remote_dns_cache_kv()?;
    let text = kv.get(&doh_cache_key(host)).text().await.ok()??;
    serde_json::from_str::<DnsHostCacheEntry>(&text)
        .ok()
        .map(|entry| entry.allowed)
}

async fn kv_put_allowed(host: &str, allowed: bool) {
    let Some(kv) = remote_dns_cache_kv() else {
        return;
    };
    let ttl = if allowed {
        DOH_CACHE_ALLOW_TTL_SECS
    } else {
        DOH_CACHE_DENY_TTL_SECS
    };
    let body = match serde_json::to_string(&DnsHostCacheEntry { allowed }) {
        Ok(body) => body,
        Err(_) => return,
    };
    let Ok(putter) = kv.put(&doh_cache_key(host), body) else {
        return;
    };
    let _ = putter.expiration_ttl(ttl).execute().await;
}

/// Validate remote fetch URL policy and DNS answers via DoH.
///
/// Cloudflare Workers `Fetch` resolves hostnames independently of this check, so
/// this is best-effort SSRF filtering rather than true IP pinning. Redirect
/// targets are re-validated before each hop.
///
/// Hostname resolution results are cached in an isolate L1 map and optional KV
/// (`REMOTE_DNS_CACHE`) so multi-hop ActivityPub fetches do not re-query DoH.
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
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if let Some(allowed) = l1_get(&host) {
        return if allowed {
            Ok(())
        } else {
            Err(Error::RustError(format!(
                "remote host {host} resolved to a blocked IP range"
            )))
        };
    }
    if let Some(allowed) = kv_get_allowed(&host).await {
        l1_put(&host, allowed);
        return if allowed {
            Ok(())
        } else {
            Err(Error::RustError(format!(
                "remote host {host} resolved to a blocked IP range"
            )))
        };
    }

    let (a_records, aaaa_records) = join(
        resolve_dns_json_ips(&host, "A"),
        resolve_dns_json_ips(&host, "AAAA"),
    )
    .await;
    let mut resolved = Vec::new();
    resolved.extend(a_records?);
    resolved.extend(aaaa_records?);
    match remote_hostname_dns_resolution_allowed(&resolved) {
        Ok(()) => {
            l1_put(&host, true);
            kv_put_allowed(&host, true).await;
            Ok(())
        }
        Err(issue) => {
            let message = match issue {
                RemoteDnsResolutionIssue::NoRecords => {
                    format!("remote host {host} did not resolve to any public A/AAAA records")
                }
                RemoteDnsResolutionIssue::BlockedAddress => {
                    // Cache blocked resolutions; NXDOMAIN / empty answers stay uncached so
                    // transient DNS failures can recover without waiting deny TTL.
                    l1_put(&host, false);
                    kv_put_allowed(&host, false).await;
                    format!("remote host {host} resolved to a blocked IP range")
                }
            };
            Err(Error::RustError(message))
        }
    }
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

#[cfg(test)]
mod tests {
    use super::doh_cache_key;

    #[test]
    fn doh_cache_key_lowercases_host() {
        assert_eq!(doh_cache_key("Fedibird.COM"), "doh:v1:fedibird.com");
    }
}
