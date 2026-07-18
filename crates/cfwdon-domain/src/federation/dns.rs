use std::net::IpAddr;

use super::url::{RemoteUrlPolicyIssue, is_blocked_ip_address, remote_url_policy_from_parts};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteDnsResolutionIssue {
    NoRecords,
    BlockedAddress,
}

/// Validates A/AAAA answers for a hostname that already passed static URL policy.
pub fn remote_hostname_dns_resolution_allowed(
    resolved_ips: &[IpAddr],
) -> Result<(), RemoteDnsResolutionIssue> {
    if resolved_ips.is_empty() {
        return Err(RemoteDnsResolutionIssue::NoRecords);
    }
    if resolved_ips.iter().copied().any(is_blocked_ip_address) {
        return Err(RemoteDnsResolutionIssue::BlockedAddress);
    }
    Ok(())
}

pub fn host_is_ip_literal(host: &str) -> bool {
    host.trim_end_matches('.').parse::<IpAddr>().is_ok()
}

pub fn parse_dns_answer_ips<'a>(answers: impl IntoIterator<Item = &'a str>) -> Vec<IpAddr> {
    answers
        .into_iter()
        .filter_map(|answer| answer.parse::<IpAddr>().ok())
        .collect()
}

/// Static host policy plus DNS answers for hostname-based SSRF defense.
pub fn remote_fetch_host_allowed(
    scheme: &str,
    host: &str,
    has_userinfo: bool,
    resolved_ips: Option<&[IpAddr]>,
) -> Result<(), RemoteFetchHostPolicyIssue> {
    remote_url_policy_from_parts(scheme, host, has_userinfo)?;
    if host_is_ip_literal(host) {
        return Ok(());
    }
    let resolved_ips = resolved_ips.ok_or(RemoteFetchHostPolicyIssue::Dns(
        RemoteDnsResolutionIssue::NoRecords,
    ))?;
    remote_hostname_dns_resolution_allowed(resolved_ips).map_err(RemoteFetchHostPolicyIssue::from)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteFetchHostPolicyIssue {
    Static(RemoteUrlPolicyIssue),
    Dns(RemoteDnsResolutionIssue),
}

impl From<RemoteUrlPolicyIssue> for RemoteFetchHostPolicyIssue {
    fn from(value: RemoteUrlPolicyIssue) -> Self {
        Self::Static(value)
    }
}

impl From<RemoteDnsResolutionIssue> for RemoteFetchHostPolicyIssue {
    fn from(value: RemoteDnsResolutionIssue) -> Self {
        Self::Dns(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn dns_resolution_rejects_private_addresses() {
        let ips = vec![
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        ];
        assert_eq!(
            remote_hostname_dns_resolution_allowed(&ips),
            Err(RemoteDnsResolutionIssue::BlockedAddress)
        );
    }

    #[test]
    fn dns_resolution_allows_public_addresses_only() {
        let ips = vec![
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
        ];
        assert!(remote_hostname_dns_resolution_allowed(&ips).is_ok());
    }

    #[test]
    fn remote_fetch_host_blocks_dns_rebinding_to_loopback() {
        let ips = vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))];
        assert_eq!(
            remote_fetch_host_allowed("https", "remote.example", false, Some(&ips)),
            Err(RemoteFetchHostPolicyIssue::Dns(
                RemoteDnsResolutionIssue::BlockedAddress
            ))
        );
    }

    #[test]
    fn literal_private_ip_is_blocked_without_dns() {
        assert_eq!(
            remote_fetch_host_allowed("https", "127.0.0.1", false, None),
            Err(RemoteFetchHostPolicyIssue::Static(
                RemoteUrlPolicyIssue::BlockedIp
            ))
        );
    }

    #[test]
    fn dns_resolution_rejects_ipv4_mapped_loopback_answers() {
        let ips = vec!["::ffff:127.0.0.1".parse().unwrap()];
        assert_eq!(
            remote_hostname_dns_resolution_allowed(&ips),
            Err(RemoteDnsResolutionIssue::BlockedAddress)
        );
    }
}
