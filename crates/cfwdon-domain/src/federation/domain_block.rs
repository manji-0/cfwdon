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

/// Mastodon's `DomainBlock#severity`, ordered from least to most strict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DomainBlockSeverity {
    /// No account-level limit; only the media and report flags apply.
    Noop,
    /// Accounts are hidden from public surfaces and their follows need approval.
    Silence,
    /// No federation at all: inbound activities, deliveries, and fetches are refused.
    Suspend,
}

impl DomainBlockSeverity {
    pub const ALL: [Self; 3] = [Self::Noop, Self::Silence, Self::Suspend];

    /// Mastodon's wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Noop => "noop",
            Self::Silence => "silence",
            Self::Suspend => "suspend",
        }
    }

    pub fn from_name(raw: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|severity| severity.as_str() == raw.trim())
    }
}

/// The enforcement part of one instance domain block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DomainBlockPolicy {
    pub severity: DomainBlockSeverity,
    pub reject_media: bool,
    pub reject_reports: bool,
}

/// What the instance refuses for one remote host.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DomainRestrictions {
    pub suspended: bool,
    pub silenced: bool,
    pub reject_media: bool,
    pub reject_reports: bool,
}

impl DomainBlockPolicy {
    /// A suspension implies every other restriction, and a silence keeps its flags.
    pub fn restrictions(self) -> DomainRestrictions {
        let suspended = self.severity == DomainBlockSeverity::Suspend;
        DomainRestrictions {
            suspended,
            silenced: self.severity >= DomainBlockSeverity::Silence,
            reject_media: suspended || self.reject_media,
            reject_reports: suspended || self.reject_reports,
        }
    }
}

impl DomainRestrictions {
    fn union(self, other: Self) -> Self {
        Self {
            suspended: self.suspended || other.suspended,
            silenced: self.silenced || other.silenced,
            reject_media: self.reject_media || other.reject_media,
            reject_reports: self.reject_reports || other.reject_reports,
        }
    }
}

/// Every restriction any block covering `host` imposes. Blocks on a domain and
/// on its parent both apply, and the strictest of each restriction wins.
pub fn domain_restrictions_for_host<'a>(
    host: &str,
    blocks: impl IntoIterator<Item = (&'a str, DomainBlockPolicy)>,
) -> DomainRestrictions {
    blocks
        .into_iter()
        .filter(|(domain, _)| host_within_blocked_domain(host, domain))
        .fold(DomainRestrictions::default(), |acc, (_, policy)| {
            acc.union(policy.restrictions())
        })
}

/// Mastodon's `DomainBlock#public_domain` for an obfuscated block: the middle
/// of the name is starred out, dots and the outer quarter on each side stay.
pub fn obfuscated_domain(domain: &str) -> String {
    let chars: Vec<char> = domain.chars().collect();
    let length = chars.len();
    let visible = length / 4;
    chars
        .iter()
        .enumerate()
        .map(|(index, &ch)| {
            if index > visible && index < length - visible && ch != '.' {
                '*'
            } else {
                ch
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        DomainBlockPolicy, DomainBlockSeverity, DomainRestrictions, domain_restrictions_for_host,
        host_within_any_blocked_domain, host_within_blocked_domain, obfuscated_domain,
    };

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

    fn policy(severity: DomainBlockSeverity, reject_media: bool) -> DomainBlockPolicy {
        DomainBlockPolicy {
            severity,
            reject_media,
            reject_reports: false,
        }
    }

    #[test]
    fn severity_sets_implied_restrictions() {
        let blocks = [
            (
                "suspended.example",
                policy(DomainBlockSeverity::Suspend, false),
            ),
            (
                "silenced.example",
                policy(DomainBlockSeverity::Silence, false),
            ),
            ("media.example", policy(DomainBlockSeverity::Noop, true)),
        ];
        assert_eq!(
            domain_restrictions_for_host("a.suspended.example", blocks),
            DomainRestrictions {
                suspended: true,
                silenced: true,
                reject_media: true,
                reject_reports: true,
            }
        );
        assert_eq!(
            domain_restrictions_for_host("silenced.example", blocks),
            DomainRestrictions {
                silenced: true,
                ..DomainRestrictions::default()
            }
        );
        assert_eq!(
            domain_restrictions_for_host("media.example", blocks),
            DomainRestrictions {
                reject_media: true,
                ..DomainRestrictions::default()
            }
        );
        assert_eq!(
            domain_restrictions_for_host("other.example", blocks),
            DomainRestrictions::default()
        );
    }

    #[test]
    fn parent_and_subdomain_blocks_combine() {
        let blocks = [
            ("example.com", policy(DomainBlockSeverity::Silence, false)),
            ("media.example.com", policy(DomainBlockSeverity::Noop, true)),
        ];
        let restrictions = domain_restrictions_for_host("media.example.com", blocks);
        assert!(restrictions.silenced && restrictions.reject_media);
        assert!(!restrictions.suspended);
    }

    #[test]
    fn severity_wire_names_round_trip() {
        for severity in DomainBlockSeverity::ALL {
            assert_eq!(
                DomainBlockSeverity::from_name(severity.as_str()),
                Some(severity)
            );
        }
        assert_eq!(DomainBlockSeverity::from_name("no_access"), None);
    }

    #[test]
    fn obfuscation_matches_mastodon_public_domain() {
        assert_eq!(obfuscated_domain("example.com"), "exa****.*om");
        assert_eq!(obfuscated_domain("badsite.social"), "bads***.***ial");
    }
}
