/// Lowercased host with any trailing root dot removed, or `None` when empty.
fn normalized_host(host: &str) -> Option<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Mastodon's domain-block rule: a block on `example.com` covers
/// `example.com` and every subdomain such as `a.example.com`, but not a
/// lookalike such as `notexample.com`.
pub fn host_within_blocked_domain(host: &str, blocked_domain: &str) -> bool {
    let (Some(host), Some(blocked)) = (normalized_host(host), normalized_host(blocked_domain))
    else {
        return false;
    };
    host == blocked
        || host
            .strip_suffix(blocked.as_str())
            .is_some_and(|prefix| prefix.ends_with('.'))
}

/// Whether any of `blocked_domains` covers `host` (see [`host_within_blocked_domain`]).
pub fn host_within_any_blocked_domain<'a>(
    host: &str,
    blocked_domains: impl IntoIterator<Item = &'a str>,
) -> bool {
    blocked_domains
        .into_iter()
        .any(|blocked| host_within_blocked_domain(host, blocked))
}

#[cfg(test)]
mod tests {
    use super::{host_within_any_blocked_domain, host_within_blocked_domain};

    #[test]
    fn block_covers_domain_and_subdomains() {
        assert!(host_within_blocked_domain("example.com", "example.com"));
        assert!(host_within_blocked_domain("a.example.com", "example.com"));
        assert!(host_within_blocked_domain("b.a.example.com", "example.com"));
        assert!(host_within_blocked_domain("A.Example.COM.", "example.com"));
        assert!(host_within_blocked_domain("a.example.com", "EXAMPLE.com"));
    }

    #[test]
    fn block_ignores_lookalikes_parents_and_empty_entries() {
        assert!(!host_within_blocked_domain("notexample.com", "example.com"));
        assert!(!host_within_blocked_domain("example.com", "a.example.com"));
        assert!(!host_within_blocked_domain(
            "example.com.evil",
            "example.com"
        ));
        assert!(!host_within_blocked_domain("example.com", ""));
        assert!(!host_within_blocked_domain("", "example.com"));
    }

    #[test]
    fn any_block_matches() {
        let blocked = ["one.example", "two.example"];
        assert!(host_within_any_blocked_domain("x.two.example", blocked));
        assert!(!host_within_any_blocked_domain("three.example", blocked));
        assert!(!host_within_any_blocked_domain("one.example", []));
    }
}
