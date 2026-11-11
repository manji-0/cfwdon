//! Mastodon's IP blocks (`IpBlock`): an admin names an IPv4 or IPv6 range and
//! what a client from it may not do. `sign_up_requires_approval` holds a new
//! account from the range for review, `sign_up_block` refuses the sign-up, and
//! `no_access` refuses every request.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

/// A CIDR range in canonical form: host bits cleared, the prefix always
/// written (`203.0.113.0/24`, `2001:db8::/32`). An IPv4-mapped IPv6 range is
/// the IPv4 range it maps, since client addresses are matched in that form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IpRange {
    V4 { network: u32, prefix: u8 },
    V6 { network: u128, prefix: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid IP range")]
pub struct InvalidIpRange;

fn mask_u32(prefix: u8) -> u32 {
    u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0)
}

fn mask_u128(prefix: u8) -> u128 {
    u128::MAX.checked_shl(128 - u32::from(prefix)).unwrap_or(0)
}

impl IpRange {
    fn new(address: IpAddr, prefix: u8) -> Self {
        match address {
            IpAddr::V4(v4) => Self::V4 {
                network: u32::from(v4) & mask_u32(prefix),
                prefix,
            },
            IpAddr::V6(v6) => Self::V6 {
                network: u128::from(v6) & mask_u128(prefix),
                prefix,
            },
        }
    }

    /// Whether `address` falls in the range. Pass addresses through
    /// [`parse_client_ip`] so an IPv4-mapped IPv6 client matches IPv4 ranges.
    pub fn contains(&self, address: IpAddr) -> bool {
        match (*self, address) {
            (Self::V4 { network, prefix }, IpAddr::V4(v4)) => {
                u32::from(v4) & mask_u32(prefix) == network
            }
            (Self::V6 { network, prefix }, IpAddr::V6(v6)) => {
                u128::from(v6) & mask_u128(prefix) == network
            }
            _ => false,
        }
    }
}

/// A range as an admin types it: an address, or an address and a prefix
/// length. A bare address is its own range (`/32`, `/128`), and host bits under
/// the prefix are cleared (`10.1.2.3/8` is `10.0.0.0/8`).
impl FromStr for IpRange {
    type Err = InvalidIpRange;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let raw = raw.trim();
        let (address, prefix) = match raw.split_once('/') {
            Some((address, prefix)) => (address, Some(prefix)),
            None => (raw, None),
        };
        let address: IpAddr = address.parse().map_err(|_| InvalidIpRange)?;
        let bits = if address.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            None => bits,
            Some(text) if !text.is_empty() && text.len() <= 3 => {
                if !text.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(InvalidIpRange);
                }
                text.parse::<u8>().map_err(|_| InvalidIpRange)?
            }
            Some(_) => return Err(InvalidIpRange),
        };
        if prefix > bits {
            return Err(InvalidIpRange);
        }
        Ok(match address {
            IpAddr::V6(v6) if prefix >= 96 => match v6.to_ipv4_mapped() {
                Some(v4) => Self::new(IpAddr::V4(v4), prefix - 96),
                None => Self::new(address, prefix),
            },
            _ => Self::new(address, prefix),
        })
    }
}

impl fmt::Display for IpRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::V4 { network, prefix } => write!(f, "{}/{prefix}", Ipv4Addr::from(network)),
            Self::V6 { network, prefix } => write!(f, "{}/{prefix}", Ipv6Addr::from(network)),
        }
    }
}

/// A client address from a trusted header. An IPv4-mapped IPv6 address
/// (`::ffff:a.b.c.d`) is the IPv4 address it maps.
pub fn parse_client_ip(raw: &str) -> Option<IpAddr> {
    let raw = raw.trim();
    let raw = raw
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(raw);
    match raw.parse::<IpAddr>().ok()? {
        IpAddr::V6(v6) => Some(v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4)),
        address => Some(address),
    }
}

/// Ordered from least to most strict, so the strictest matching block wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IpBlockSeverity {
    SignUpRequiresApproval,
    SignUpBlock,
    NoAccess,
}

impl IpBlockSeverity {
    pub const ALL: [Self; 3] = [
        Self::SignUpRequiresApproval,
        Self::SignUpBlock,
        Self::NoAccess,
    ];

    /// Mastodon's wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SignUpRequiresApproval => "sign_up_requires_approval",
            Self::SignUpBlock => "sign_up_block",
            Self::NoAccess => "no_access",
        }
    }

    pub fn from_name(raw: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|severity| severity.as_str() == raw.trim())
    }
}

/// One block as enforcement reads it. `expires_at_unix` is `None` for a
/// permanent block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpBlockRule {
    pub range: IpRange,
    pub severity: IpBlockSeverity,
    pub expires_at_unix: Option<i64>,
}

impl IpBlockRule {
    pub fn is_active(&self, now_unix: i64) -> bool {
        self.expires_at_unix.is_none_or(|at| at > now_unix)
    }
}

/// The strictest block in force that covers `address`.
pub fn strictest_ip_block(
    rules: &[IpBlockRule],
    address: IpAddr,
    now_unix: i64,
) -> Option<IpBlockSeverity> {
    rules
        .iter()
        .filter(|rule| rule.is_active(now_unix) && rule.range.contains(address))
        .map(|rule| rule.severity)
        .max()
}

/// What a sign-up from an address may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IpSignUpDecision {
    Open,
    RequiresApproval,
    Blocked,
}

impl IpSignUpDecision {
    pub const ALL: [Self; 3] = [Self::Open, Self::RequiresApproval, Self::Blocked];

    pub fn for_block(block: Option<IpBlockSeverity>) -> Self {
        match block {
            None => Self::Open,
            Some(IpBlockSeverity::SignUpRequiresApproval) => Self::RequiresApproval,
            Some(IpBlockSeverity::SignUpBlock | IpBlockSeverity::NoAccess) => Self::Blocked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        IpBlockRule, IpBlockSeverity, IpRange, IpSignUpDecision, parse_client_ip,
        strictest_ip_block,
    };

    fn range(raw: &str) -> IpRange {
        raw.parse().unwrap()
    }

    fn ip(raw: &str) -> std::net::IpAddr {
        parse_client_ip(raw).unwrap()
    }

    #[test]
    fn ranges_parse_into_canonical_cidr() {
        for (input, canonical) in [
            ("203.0.113.7", "203.0.113.7/32"),
            ("10.1.2.3/8", "10.0.0.0/8"),
            (" 192.0.2.0/24 ", "192.0.2.0/24"),
            ("0.0.0.0/0", "0.0.0.0/0"),
            ("2001:DB8::1/32", "2001:db8::/32"),
            ("2001:db8::1", "2001:db8::1/128"),
            ("::ffff:192.0.2.9/120", "192.0.2.0/24"),
            ("::/0", "::/0"),
        ] {
            assert_eq!(range(input).to_string(), canonical, "{input}");
        }
    }

    #[test]
    fn malformed_ranges_are_rejected() {
        for input in [
            "",
            "example.com",
            "10.0.0.0/33",
            "2001:db8::/129",
            "10.0.0.0/",
            "10.0.0.0/+8",
            "10.0.0.0/8/8",
            "010.0.0.1",
            "1.2.3",
            "1:2:3:4:5:6:7:8:9",
        ] {
            assert!(input.parse::<IpRange>().is_err(), "{input}");
        }
    }

    #[test]
    fn containment_covers_both_families_and_mapped_addresses() {
        let v4 = range("192.0.2.0/24");
        assert!(v4.contains(ip("192.0.2.200")));
        assert!(v4.contains(ip("::ffff:192.0.2.1")));
        assert!(!v4.contains(ip("192.0.3.1")));
        assert!(!v4.contains(ip("2001:db8::1")));

        let v6 = range("2001:db8:abcd::/48");
        assert!(v6.contains(ip("2001:db8:abcd:12::1")));
        assert!(v6.contains(ip("[2001:db8:abcd::5]")));
        assert!(!v6.contains(ip("2001:db8:abce::1")));
        assert!(!v6.contains(ip("192.0.2.1")));

        assert!(range("0.0.0.0/0").contains(ip("8.8.8.8")));
        assert!(range("::/0").contains(ip("2001:db8::1")));
        assert!(parse_client_ip("not an ip").is_none());
    }

    #[test]
    fn strictest_active_block_decides() {
        let rules = [
            IpBlockRule {
                range: range("192.0.2.0/24"),
                severity: IpBlockSeverity::SignUpRequiresApproval,
                expires_at_unix: None,
            },
            IpBlockRule {
                range: range("192.0.2.128/25"),
                severity: IpBlockSeverity::NoAccess,
                expires_at_unix: Some(1_000),
            },
        ];
        assert_eq!(
            strictest_ip_block(&rules, ip("192.0.2.200"), 999),
            Some(IpBlockSeverity::NoAccess)
        );
        assert_eq!(
            strictest_ip_block(&rules, ip("192.0.2.200"), 1_000),
            Some(IpBlockSeverity::SignUpRequiresApproval),
            "an expired block no longer applies"
        );
        assert_eq!(strictest_ip_block(&rules, ip("198.51.100.1"), 0), None);
    }

    #[test]
    fn sign_up_decision_follows_severity() {
        assert_eq!(IpSignUpDecision::for_block(None), IpSignUpDecision::Open);
        assert_eq!(
            IpSignUpDecision::for_block(Some(IpBlockSeverity::SignUpRequiresApproval)),
            IpSignUpDecision::RequiresApproval
        );
        assert_eq!(
            IpSignUpDecision::for_block(Some(IpBlockSeverity::SignUpBlock)),
            IpSignUpDecision::Blocked
        );
        assert_eq!(
            IpSignUpDecision::for_block(Some(IpBlockSeverity::NoAccess)),
            IpSignUpDecision::Blocked
        );
    }

    #[test]
    fn severity_wire_names_round_trip() {
        for severity in IpBlockSeverity::ALL {
            assert_eq!(
                IpBlockSeverity::from_name(severity.as_str()),
                Some(severity)
            );
        }
        assert_eq!(IpBlockSeverity::from_name("suspend"), None);
    }
}
