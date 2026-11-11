use cfwdon_domain::{
    ACTIVITYPUB_MAX_DATE_AGE_MS, ACTIVITYPUB_MAX_DATE_AHEAD_MS, DomainBlockPolicy,
    DomainBlockSeverity, RemoteUrlPolicyIssue, activitypub_date_within_skew,
    activitypub_key_id_matches_actor, activitypub_signature_lists_required_headers,
    cached_remote_actor_key_matches, domain_restrictions_for_host, remote_url_policy_from_parts,
};
use stateright::{Checker, Model, Property};

#[derive(Clone, Copy, Debug)]
pub(crate) struct FederationRequestPolicyModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SignedHeaderSet {
    MissingTarget,
    MissingDigest,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum KeyIdCase {
    MatchingFragment,
    MatchingCanonical,
    MismatchedActor,
    InvalidScheme,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RemoteHostCase {
    Public,
    Localhost,
    PrivateIp,
    UserInfo,
    UnsupportedScheme,
}

/// Remote host against a `suspend` block on `blocked.example` and a `silence`
/// block on `silenced.example`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum DomainBlockCase {
    Unblocked,
    BlockedDomain,
    BlockedSubdomain,
    Lookalike,
    Silenced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum DateSkewCase {
    Now,
    OldestAccepted,
    FarthestAheadAccepted,
    TooOld,
    TooFarAhead,
}

impl DateSkewCase {
    fn is_within(self) -> bool {
        matches!(
            self,
            Self::Now | Self::OldestAccepted | Self::FarthestAheadAccepted
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FederationRequestPolicyModelState {
    signed_headers: SignedHeaderSet,
    key_id_case: KeyIdCase,
    cached_public_key_id_present: bool,
    remote_host: RemoteHostCase,
    date_skew: DateSkewCase,
    domain_block: DomainBlockCase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FederationRequestPolicyAction {
    CycleSignedHeaders,
    CycleKeyIdCase,
    ToggleCachedPublicKeyId,
    CycleRemoteHost,
    CycleDateSkew,
    CycleDomainBlock,
}

impl FederationRequestPolicyModel {
    const ACTOR_URI: &'static str = "https://remote.example/users/bob";
    const CANONICAL_ACTOR_URI: &'static str = "https://remote.example/@bob";
    const DOMAIN_BLOCKS: [(&'static str, DomainBlockPolicy); 2] = [
        (
            "blocked.example",
            DomainBlockPolicy {
                severity: DomainBlockSeverity::Suspend,
                reject_media: false,
                reject_reports: false,
            },
        ),
        (
            "silenced.example",
            DomainBlockPolicy {
                severity: DomainBlockSeverity::Silence,
                reject_media: true,
                reject_reports: true,
            },
        ),
    ];

    fn signed_header_list(set: SignedHeaderSet) -> Vec<String> {
        match set {
            SignedHeaderSet::MissingTarget => {
                vec!["date".to_owned(), "digest".to_owned()]
            }
            SignedHeaderSet::MissingDigest => {
                vec!["(request-target)".to_owned(), "date".to_owned()]
            }
            SignedHeaderSet::Complete => vec![
                "(request-target)".to_owned(),
                "host".to_owned(),
                "date".to_owned(),
                "digest".to_owned(),
            ],
        }
    }

    fn key_id(case: KeyIdCase) -> &'static str {
        match case {
            KeyIdCase::MatchingFragment => "https://remote.example/users/bob#main-key",
            KeyIdCase::MatchingCanonical => "https://remote.example/@bob",
            KeyIdCase::MismatchedActor => "https://remote.example/users/eve#main-key",
            KeyIdCase::InvalidScheme => "ftp://remote.example/users/bob#main-key",
        }
    }

    fn remote_url_parts(case: RemoteHostCase) -> (&'static str, &'static str, bool) {
        match case {
            RemoteHostCase::Public => ("https", "remote.example", false),
            RemoteHostCase::Localhost => ("https", "localhost", false),
            RemoteHostCase::PrivateIp => ("https", "127.0.0.1", false),
            RemoteHostCase::UserInfo => ("https", "remote.example", true),
            RemoteHostCase::UnsupportedScheme => ("file", "remote.example", false),
        }
    }

    fn domain_block_host(case: DomainBlockCase) -> &'static str {
        match case {
            DomainBlockCase::Unblocked => "remote.example",
            DomainBlockCase::BlockedDomain => "blocked.example",
            DomainBlockCase::BlockedSubdomain => "a.blocked.example",
            DomainBlockCase::Lookalike => "notblocked.example",
            DomainBlockCase::Silenced => "a.silenced.example",
        }
    }

    fn cycle_domain_block(current: DomainBlockCase) -> DomainBlockCase {
        match current {
            DomainBlockCase::Unblocked => DomainBlockCase::BlockedDomain,
            DomainBlockCase::BlockedDomain => DomainBlockCase::BlockedSubdomain,
            DomainBlockCase::BlockedSubdomain => DomainBlockCase::Lookalike,
            DomainBlockCase::Lookalike => DomainBlockCase::Silenced,
            DomainBlockCase::Silenced => DomainBlockCase::Unblocked,
        }
    }

    fn cycle_signed_headers(current: SignedHeaderSet) -> SignedHeaderSet {
        match current {
            SignedHeaderSet::MissingTarget => SignedHeaderSet::MissingDigest,
            SignedHeaderSet::MissingDigest => SignedHeaderSet::Complete,
            SignedHeaderSet::Complete => SignedHeaderSet::MissingTarget,
        }
    }

    fn cycle_key_id_case(current: KeyIdCase) -> KeyIdCase {
        match current {
            KeyIdCase::MatchingFragment => KeyIdCase::MatchingCanonical,
            KeyIdCase::MatchingCanonical => KeyIdCase::MismatchedActor,
            KeyIdCase::MismatchedActor => KeyIdCase::InvalidScheme,
            KeyIdCase::InvalidScheme => KeyIdCase::MatchingFragment,
        }
    }

    fn cycle_remote_host(current: RemoteHostCase) -> RemoteHostCase {
        match current {
            RemoteHostCase::Public => RemoteHostCase::Localhost,
            RemoteHostCase::Localhost => RemoteHostCase::PrivateIp,
            RemoteHostCase::PrivateIp => RemoteHostCase::UserInfo,
            RemoteHostCase::UserInfo => RemoteHostCase::UnsupportedScheme,
            RemoteHostCase::UnsupportedScheme => RemoteHostCase::Public,
        }
    }

    fn signature_headers_valid(state: &FederationRequestPolicyModelState) -> bool {
        activitypub_signature_lists_required_headers(&Self::signed_header_list(
            state.signed_headers,
        ))
    }

    fn key_id_matches_actor(state: &FederationRequestPolicyModelState) -> bool {
        activitypub_key_id_matches_actor(
            Self::key_id(state.key_id_case),
            Self::ACTOR_URI,
            Self::CANONICAL_ACTOR_URI,
        )
    }

    fn cached_key_matches(state: &FederationRequestPolicyModelState) -> bool {
        let cached_public_key_id = if state.cached_public_key_id_present {
            Self::key_id(state.key_id_case)
        } else {
            ""
        };
        cached_remote_actor_key_matches(
            Self::key_id_matches_actor(state),
            cached_public_key_id,
            Self::key_id(state.key_id_case),
        )
    }

    fn remote_url_allowed(state: &FederationRequestPolicyModelState) -> bool {
        let (scheme, host, has_userinfo) = Self::remote_url_parts(state.remote_host);
        remote_url_policy_from_parts(scheme, host, has_userinfo).is_ok()
    }

    fn domain_suspended(state: &FederationRequestPolicyModelState) -> bool {
        domain_restrictions_for_host(
            Self::domain_block_host(state.domain_block),
            Self::DOMAIN_BLOCKS,
        )
        .suspended
    }

    fn date_within_skew(state: &FederationRequestPolicyModelState) -> bool {
        let now_ms = 1_000_000.0;
        let parsed_ms = match state.date_skew {
            DateSkewCase::Now => now_ms,
            DateSkewCase::OldestAccepted => now_ms - ACTIVITYPUB_MAX_DATE_AGE_MS,
            DateSkewCase::FarthestAheadAccepted => now_ms + ACTIVITYPUB_MAX_DATE_AHEAD_MS,
            DateSkewCase::TooOld => now_ms - ACTIVITYPUB_MAX_DATE_AGE_MS - 1.0,
            DateSkewCase::TooFarAhead => now_ms + ACTIVITYPUB_MAX_DATE_AHEAD_MS + 1.0,
        };
        activitypub_date_within_skew(parsed_ms, now_ms)
    }
}

impl Model for FederationRequestPolicyModel {
    type State = FederationRequestPolicyModelState;
    type Action = FederationRequestPolicyAction;

    fn init_states(&self) -> Vec<Self::State> {
        let mut states = Vec::new();

        for signed_headers in [
            SignedHeaderSet::MissingTarget,
            SignedHeaderSet::MissingDigest,
            SignedHeaderSet::Complete,
        ] {
            for key_id_case in [
                KeyIdCase::MatchingFragment,
                KeyIdCase::MatchingCanonical,
                KeyIdCase::MismatchedActor,
                KeyIdCase::InvalidScheme,
            ] {
                for cached_public_key_id_present in [false, true] {
                    for remote_host in [
                        RemoteHostCase::Public,
                        RemoteHostCase::Localhost,
                        RemoteHostCase::PrivateIp,
                        RemoteHostCase::UserInfo,
                        RemoteHostCase::UnsupportedScheme,
                    ] {
                        for domain_block in [
                            DomainBlockCase::Unblocked,
                            DomainBlockCase::BlockedDomain,
                            DomainBlockCase::BlockedSubdomain,
                            DomainBlockCase::Lookalike,
                            DomainBlockCase::Silenced,
                        ] {
                            states.push(FederationRequestPolicyModelState {
                                signed_headers,
                                key_id_case,
                                cached_public_key_id_present,
                                remote_host,
                                date_skew: DateSkewCase::Now,
                                domain_block,
                            });
                        }
                    }
                }
            }
        }

        states
    }

    fn actions(&self, _state: &Self::State, actions: &mut Vec<Self::Action>) {
        actions.extend([
            FederationRequestPolicyAction::CycleSignedHeaders,
            FederationRequestPolicyAction::CycleKeyIdCase,
            FederationRequestPolicyAction::ToggleCachedPublicKeyId,
            FederationRequestPolicyAction::CycleRemoteHost,
            FederationRequestPolicyAction::CycleDateSkew,
            FederationRequestPolicyAction::CycleDomainBlock,
        ]);
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;

        match action {
            FederationRequestPolicyAction::CycleSignedHeaders => {
                next.signed_headers = Self::cycle_signed_headers(next.signed_headers);
            }
            FederationRequestPolicyAction::CycleKeyIdCase => {
                next.key_id_case = Self::cycle_key_id_case(next.key_id_case);
            }
            FederationRequestPolicyAction::ToggleCachedPublicKeyId => {
                next.cached_public_key_id_present = !next.cached_public_key_id_present;
            }
            FederationRequestPolicyAction::CycleRemoteHost => {
                next.remote_host = Self::cycle_remote_host(next.remote_host);
            }
            FederationRequestPolicyAction::CycleDateSkew => {
                next.date_skew = match next.date_skew {
                    DateSkewCase::Now => DateSkewCase::OldestAccepted,
                    DateSkewCase::OldestAccepted => DateSkewCase::FarthestAheadAccepted,
                    DateSkewCase::FarthestAheadAccepted => DateSkewCase::TooOld,
                    DateSkewCase::TooOld => DateSkewCase::TooFarAhead,
                    DateSkewCase::TooFarAhead => DateSkewCase::Now,
                };
            }
            FederationRequestPolicyAction::CycleDomainBlock => {
                next.domain_block = Self::cycle_domain_block(next.domain_block);
            }
        }

        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "complete_signed_headers_are_valid",
                |_, state: &FederationRequestPolicyModelState| {
                    state.signed_headers != SignedHeaderSet::Complete
                        || FederationRequestPolicyModel::signature_headers_valid(state)
                },
            ),
            Property::always(
                "incomplete_signed_headers_are_invalid",
                |_, state: &FederationRequestPolicyModelState| {
                    state.signed_headers == SignedHeaderSet::Complete
                        || !FederationRequestPolicyModel::signature_headers_valid(state)
                },
            ),
            Property::always(
                "matching_key_ids_accept_fragment_or_canonical_actor",
                |_, state: &FederationRequestPolicyModelState| {
                    !matches!(
                        state.key_id_case,
                        KeyIdCase::MatchingFragment | KeyIdCase::MatchingCanonical
                    ) || FederationRequestPolicyModel::key_id_matches_actor(state)
                },
            ),
            Property::always(
                "mismatched_or_invalid_key_ids_reject",
                |_, state: &FederationRequestPolicyModelState| {
                    !matches!(
                        state.key_id_case,
                        KeyIdCase::MismatchedActor | KeyIdCase::InvalidScheme
                    ) || !FederationRequestPolicyModel::key_id_matches_actor(state)
                },
            ),
            Property::always(
                "cached_key_requires_actor_match",
                |_, state: &FederationRequestPolicyModelState| {
                    !FederationRequestPolicyModel::cached_key_matches(state)
                        || FederationRequestPolicyModel::key_id_matches_actor(state)
                },
            ),
            Property::always(
                "public_remote_host_is_allowed",
                |_, state: &FederationRequestPolicyModelState| {
                    state.remote_host != RemoteHostCase::Public
                        || FederationRequestPolicyModel::remote_url_allowed(state)
                },
            ),
            Property::always(
                "blocked_remote_hosts_are_rejected",
                |_, state: &FederationRequestPolicyModelState| {
                    matches!(state.remote_host, RemoteHostCase::Public)
                        || !FederationRequestPolicyModel::remote_url_allowed(state)
                },
            ),
            Property::always(
                "localhost_remote_host_is_blocked",
                |_, state: &FederationRequestPolicyModelState| {
                    state.remote_host != RemoteHostCase::Localhost
                        || remote_url_policy_from_parts("https", "localhost", false)
                            == Err(RemoteUrlPolicyIssue::LocalhostBlocked)
                },
            ),
            Property::always(
                "within_skew_dates_are_accepted",
                |_, state: &FederationRequestPolicyModelState| {
                    !state.date_skew.is_within()
                        || FederationRequestPolicyModel::date_within_skew(state)
                },
            ),
            Property::always(
                "outside_skew_dates_are_rejected",
                |_, state: &FederationRequestPolicyModelState| {
                    state.date_skew.is_within()
                        || !FederationRequestPolicyModel::date_within_skew(state)
                },
            ),
            Property::always(
                "suspended_domain_and_subdomains_are_refused",
                |_, state: &FederationRequestPolicyModelState| {
                    !matches!(
                        state.domain_block,
                        DomainBlockCase::BlockedDomain | DomainBlockCase::BlockedSubdomain
                    ) || FederationRequestPolicyModel::domain_suspended(state)
                },
            ),
            Property::always(
                "unblocked_lookalike_and_silenced_domains_federate",
                |_, state: &FederationRequestPolicyModelState| {
                    !matches!(
                        state.domain_block,
                        DomainBlockCase::Unblocked
                            | DomainBlockCase::Lookalike
                            | DomainBlockCase::Silenced
                    ) || !FederationRequestPolicyModel::domain_suspended(state)
                },
            ),
            Property::sometimes(
                "valid_signature_prerequisites_reachable",
                |_, state: &FederationRequestPolicyModelState| {
                    FederationRequestPolicyModel::signature_headers_valid(state)
                        && FederationRequestPolicyModel::key_id_matches_actor(state)
                        && FederationRequestPolicyModel::remote_url_allowed(state)
                        && FederationRequestPolicyModel::date_within_skew(state)
                        && !FederationRequestPolicyModel::domain_suspended(state)
                },
            ),
        ]
    }
}

pub(crate) fn check_federation_request_policy_model() {
    FederationRequestPolicyModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_federation_request_policy_model;

    #[test]
    fn federation_request_policy_model_holds() {
        check_federation_request_policy_model();
    }
}
