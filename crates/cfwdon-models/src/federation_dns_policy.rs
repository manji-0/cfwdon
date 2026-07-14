use std::net::{IpAddr, Ipv4Addr};

use cfwdon_domain::{
    RemoteDnsResolutionIssue, RemoteUrlPolicyIssue, host_is_ip_literal, remote_fetch_host_allowed,
    remote_hostname_dns_resolution_allowed, remote_url_policy_from_parts,
};
use stateright::{Checker, Model, Property};

#[derive(Clone, Copy, Debug)]
pub(crate) struct FederationDnsPolicyModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StaticHostCase {
    PublicHostname,
    Localhost,
    LiteralPublicIp,
    LiteralPrivateIp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum DnsResolutionCase {
    NotRequired,
    PublicOnly,
    PrivateOnly,
    Mixed,
    NoRecords,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FederationDnsPolicyModelState {
    static_host: StaticHostCase,
    dns_resolution: DnsResolutionCase,
    cache_hit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FederationDnsPolicyAction {
    CycleStaticHost,
    CycleDnsResolution,
    ToggleCacheHit,
}

impl FederationDnsPolicyModel {
    const PUBLIC_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));
    const PRIVATE_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

    fn static_host_value(case: StaticHostCase) -> &'static str {
        match case {
            StaticHostCase::PublicHostname => "remote.example",
            StaticHostCase::Localhost => "localhost",
            StaticHostCase::LiteralPublicIp => "8.8.8.8",
            StaticHostCase::LiteralPrivateIp => "127.0.0.1",
        }
    }

    fn cycle_static_host(current: StaticHostCase) -> StaticHostCase {
        match current {
            StaticHostCase::PublicHostname => StaticHostCase::Localhost,
            StaticHostCase::Localhost => StaticHostCase::LiteralPublicIp,
            StaticHostCase::LiteralPublicIp => StaticHostCase::LiteralPrivateIp,
            StaticHostCase::LiteralPrivateIp => StaticHostCase::PublicHostname,
        }
    }

    fn cycle_dns_resolution(current: DnsResolutionCase) -> DnsResolutionCase {
        match current {
            DnsResolutionCase::NotRequired => DnsResolutionCase::PublicOnly,
            DnsResolutionCase::PublicOnly => DnsResolutionCase::PrivateOnly,
            DnsResolutionCase::PrivateOnly => DnsResolutionCase::Mixed,
            DnsResolutionCase::Mixed => DnsResolutionCase::NoRecords,
            DnsResolutionCase::NoRecords => DnsResolutionCase::NotRequired,
        }
    }

    fn effective_dns_resolution(state: &FederationDnsPolicyModelState) -> DnsResolutionCase {
        if host_is_ip_literal(Self::static_host_value(state.static_host)) {
            DnsResolutionCase::NotRequired
        } else {
            state.dns_resolution
        }
    }

    fn resolved_ips(case: DnsResolutionCase) -> Option<&'static [IpAddr]> {
        match case {
            DnsResolutionCase::NotRequired => None,
            DnsResolutionCase::PublicOnly => Some(&[Self::PUBLIC_IP]),
            DnsResolutionCase::PrivateOnly => Some(&[Self::PRIVATE_IP]),
            DnsResolutionCase::Mixed => Some(&[Self::PUBLIC_IP, Self::PRIVATE_IP]),
            DnsResolutionCase::NoRecords => Some(&[]),
        }
    }

    fn worker_validates_dns(state: &FederationDnsPolicyModelState) -> bool {
        !host_is_ip_literal(Self::static_host_value(state.static_host)) && !state.cache_hit
    }

    fn fetch_allowed(state: &FederationDnsPolicyModelState) -> bool {
        let host = Self::static_host_value(state.static_host);
        if state.cache_hit && !host_is_ip_literal(host) {
            return remote_url_policy_from_parts("https", host, false).is_ok();
        }

        let dns_case = Self::effective_dns_resolution(state);
        remote_fetch_host_allowed("https", host, false, Self::resolved_ips(dns_case)).is_ok()
    }

    fn static_policy_issue(state: &FederationDnsPolicyModelState) -> Option<RemoteUrlPolicyIssue> {
        remote_url_policy_from_parts("https", Self::static_host_value(state.static_host), false)
            .err()
    }

    fn dns_policy_issue(state: &FederationDnsPolicyModelState) -> Option<RemoteDnsResolutionIssue> {
        if !Self::worker_validates_dns(state) {
            return None;
        }
        let dns_case = Self::effective_dns_resolution(state);
        remote_hostname_dns_resolution_allowed(Self::resolved_ips(dns_case).unwrap_or_default())
            .err()
    }
}

impl Model for FederationDnsPolicyModel {
    type State = FederationDnsPolicyModelState;
    type Action = FederationDnsPolicyAction;

    fn init_states(&self) -> Vec<Self::State> {
        let mut states = Vec::new();
        for static_host in [
            StaticHostCase::PublicHostname,
            StaticHostCase::Localhost,
            StaticHostCase::LiteralPublicIp,
            StaticHostCase::LiteralPrivateIp,
        ] {
            for dns_resolution in [
                DnsResolutionCase::NotRequired,
                DnsResolutionCase::PublicOnly,
                DnsResolutionCase::PrivateOnly,
                DnsResolutionCase::Mixed,
                DnsResolutionCase::NoRecords,
            ] {
                for cache_hit in [false, true] {
                    states.push(FederationDnsPolicyModelState {
                        static_host,
                        dns_resolution,
                        cache_hit,
                    });
                }
            }
        }
        states
    }

    fn actions(&self, _state: &Self::State, actions: &mut Vec<Self::Action>) {
        actions.extend([
            FederationDnsPolicyAction::CycleStaticHost,
            FederationDnsPolicyAction::CycleDnsResolution,
            FederationDnsPolicyAction::ToggleCacheHit,
        ]);
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;
        match action {
            FederationDnsPolicyAction::CycleStaticHost => {
                next.static_host = Self::cycle_static_host(next.static_host);
            }
            FederationDnsPolicyAction::CycleDnsResolution => {
                next.dns_resolution = Self::cycle_dns_resolution(next.dns_resolution);
            }
            FederationDnsPolicyAction::ToggleCacheHit => {
                next.cache_hit = !next.cache_hit;
            }
        }
        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "literal_private_ip_blocked_without_dns",
                |_, state: &FederationDnsPolicyModelState| {
                    state.static_host != StaticHostCase::LiteralPrivateIp
                        || !FederationDnsPolicyModel::fetch_allowed(state)
                },
            ),
            Property::always(
                "literal_public_ip_allowed_without_dns",
                |_, state: &FederationDnsPolicyModelState| {
                    state.static_host != StaticHostCase::LiteralPublicIp
                        || FederationDnsPolicyModel::fetch_allowed(state)
                },
            ),
            Property::always(
                "localhost_blocked_before_dns",
                |_, state: &FederationDnsPolicyModelState| {
                    state.static_host != StaticHostCase::Localhost
                        || FederationDnsPolicyModel::static_policy_issue(state)
                            == Some(RemoteUrlPolicyIssue::LocalhostBlocked)
                },
            ),
            Property::always(
                "public_hostname_public_dns_allowed",
                |_, state: &FederationDnsPolicyModelState| {
                    state.static_host != StaticHostCase::PublicHostname
                        || FederationDnsPolicyModel::effective_dns_resolution(state)
                            != DnsResolutionCase::PublicOnly
                        || state.cache_hit
                        || FederationDnsPolicyModel::fetch_allowed(state)
                },
            ),
            Property::always(
                "dns_rebinding_private_resolution_blocked",
                |_, state: &FederationDnsPolicyModelState| {
                    state.static_host != StaticHostCase::PublicHostname
                        || !matches!(
                            FederationDnsPolicyModel::effective_dns_resolution(state),
                            DnsResolutionCase::PrivateOnly | DnsResolutionCase::Mixed
                        )
                        || state.cache_hit
                        || !FederationDnsPolicyModel::fetch_allowed(state)
                },
            ),
            Property::always(
                "empty_dns_answers_blocked",
                |_, state: &FederationDnsPolicyModelState| {
                    state.static_host != StaticHostCase::PublicHostname
                        || FederationDnsPolicyModel::effective_dns_resolution(state)
                            != DnsResolutionCase::NoRecords
                        || state.cache_hit
                        || FederationDnsPolicyModel::dns_policy_issue(state)
                            == Some(RemoteDnsResolutionIssue::NoRecords)
                },
            ),
            Property::always(
                "cache_hit_skips_dns_validation",
                |_, state: &FederationDnsPolicyModelState| {
                    !state.cache_hit
                        || state.static_host != StaticHostCase::PublicHostname
                        || FederationDnsPolicyModel::dns_policy_issue(state).is_none()
                },
            ),
            Property::always(
                "fetch_allowed_matches_domain_composition",
                |_, state: &FederationDnsPolicyModelState| {
                    let host = FederationDnsPolicyModel::static_host_value(state.static_host);
                    let expected = if state.cache_hit && !host_is_ip_literal(host) {
                        remote_url_policy_from_parts("https", host, false).is_ok()
                    } else {
                        remote_fetch_host_allowed(
                            "https",
                            host,
                            false,
                            FederationDnsPolicyModel::resolved_ips(
                                FederationDnsPolicyModel::effective_dns_resolution(state),
                            ),
                        )
                        .is_ok()
                    };
                    FederationDnsPolicyModel::fetch_allowed(state) == expected
                },
            ),
            Property::sometimes(
                "public_hostname_fetch_reachable",
                |_, state: &FederationDnsPolicyModelState| {
                    state.static_host == StaticHostCase::PublicHostname
                        && FederationDnsPolicyModel::fetch_allowed(state)
                },
            ),
            Property::sometimes(
                "dns_rebinding_blocked_reachable",
                |_, state: &FederationDnsPolicyModelState| {
                    state.static_host == StaticHostCase::PublicHostname
                        && matches!(
                            FederationDnsPolicyModel::effective_dns_resolution(state),
                            DnsResolutionCase::PrivateOnly | DnsResolutionCase::Mixed
                        )
                        && !state.cache_hit
                        && !FederationDnsPolicyModel::fetch_allowed(state)
                },
            ),
        ]
    }
}

pub(crate) fn check_federation_dns_policy_model() {
    FederationDnsPolicyModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_federation_dns_policy_model;

    #[test]
    fn federation_dns_policy_model_holds() {
        check_federation_dns_policy_model();
    }
}
