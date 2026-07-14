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
}
