use std::net::IpAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteUrlPolicyIssue {
    UnsupportedScheme,
    MissingHost,
    UserInfoPresent,
    LocalhostBlocked,
    BlockedIp,
}

pub fn remote_http_url_scheme_allowed(url: &str) -> bool {
    let trimmed = url.trim();
    trimmed.starts_with("http://") || trimmed.starts_with("https://")
}

pub fn remote_url_policy_from_parts(
    scheme: &str,
    host: &str,
    has_userinfo: bool,
) -> Result<(), RemoteUrlPolicyIssue> {
    if !matches!(scheme, "http" | "https") {
        return Err(RemoteUrlPolicyIssue::UnsupportedScheme);
    }
    if has_userinfo {
        return Err(RemoteUrlPolicyIssue::UserInfoPresent);
    }
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return Err(RemoteUrlPolicyIssue::MissingHost);
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return Err(RemoteUrlPolicyIssue::LocalhostBlocked);
    }
    if let Ok(ip) = host.parse::<IpAddr>()
        && is_blocked_ip_address(ip)
    {
        return Err(RemoteUrlPolicyIssue::BlockedIp);
    }
    Ok(())
}

pub fn is_blocked_ip_address(ip: IpAddr) -> bool {
    let ip = normalize_ip_for_blocklist(ip);
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.is_broadcast()
                || v4.octets()[0] == 0
                || is_blocked_cgnat_v4(v4)
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

fn normalize_ip_for_blocklist(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        other => other,
    }
}

fn is_blocked_cgnat_v4(v4: std::net::Ipv4Addr) -> bool {
    let [octet0, octet1, _, _] = v4.octets();
    octet0 == 100 && (64..=127).contains(&octet1)
}

pub fn remote_url_policy_for_ip(
    ip: IpAddr,
    has_userinfo: bool,
) -> Result<(), RemoteUrlPolicyIssue> {
    if has_userinfo {
        return Err(RemoteUrlPolicyIssue::UserInfoPresent);
    }
    if is_blocked_ip_address(ip) {
        return Err(RemoteUrlPolicyIssue::BlockedIp);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_url_policy_rejects_localhost_and_private_ips() {
        assert_eq!(
            remote_url_policy_from_parts("https", "localhost", false),
            Err(RemoteUrlPolicyIssue::LocalhostBlocked)
        );
        assert_eq!(
            remote_url_policy_from_parts("https", "127.0.0.1", false),
            Err(RemoteUrlPolicyIssue::BlockedIp)
        );
        assert_eq!(
            remote_url_policy_from_parts("https", "remote.example", false),
            Ok(())
        );
    }

    #[test]
    fn blocked_ip_address_rejects_ipv4_mapped_loopback() {
        assert!(is_blocked_ip_address("::ffff:127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn blocked_ip_address_rejects_cgnat_range() {
        assert!(is_blocked_ip_address("100.64.0.1".parse().unwrap()));
    }

    #[test]
    fn remote_url_policy_for_ip_rejects_mapped_private_addresses() {
        assert_eq!(
            remote_url_policy_for_ip("::ffff:10.0.0.1".parse().unwrap(), false),
            Err(RemoteUrlPolicyIssue::BlockedIp)
        );
    }
}
