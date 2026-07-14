/// Static metadata linking a Stateright model to domain symbols and worker call sites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefinementEntry {
    pub model: &'static str,
    pub domain_module: &'static str,
    pub implementation_sites: &'static [&'static str],
    pub abstraction: &'static str,
    pub operations: &'static [OperationMapping],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationMapping {
    pub model_action: &'static str,
    pub domain_call: &'static str,
    pub implementation_call: &'static str,
    pub worker_guard: &'static str,
}

pub const REFINEMENT_CATALOG: &[RefinementEntry] = &[
    RefinementEntry {
        model: "quote",
        domain_module: "cfwdon_domain::quote",
        implementation_sites: &[
            "crates/cfwdon-worker/src/statuses/mutations.rs::insert_status",
            "crates/cfwdon-domain/src/status/draft.rs::PublishIntent",
        ],
        abstraction: "quote_state + visibility + policy + quote-target facts",
        operations: &[
            OperationMapping {
                model_action: "ResolveInitial",
                domain_call: "QuoteState::initial_for_quote_target",
                implementation_call: "StatusDraft::into_publish_intent → quote_target.initial_state",
                worker_guard: "always when publishing",
            },
            OperationMapping {
                model_action: "ResolveRemote",
                domain_call: "QuoteState::remote_for_target",
                implementation_call: "remote status store intent builder",
                worker_guard: "remote ActivityPub ingest",
            },
            OperationMapping {
                model_action: "ApplyVisibilityPolicy",
                domain_call: "QuoteApprovalPolicy::for_status_visibility",
                implementation_call: "insert_status / publish intent",
                worker_guard: "always on publish",
            },
            OperationMapping {
                model_action: "Revoke",
                domain_call: "QuoteState::quote_state_after_revoke",
                implementation_call: "meta_placeholder_routes::revoke_quote_response",
                worker_guard: "requester is quote author",
            },
        ],
    },
    RefinementEntry {
        model: "quote_approval",
        domain_module: "cfwdon_domain::quote",
        implementation_sites: &[
            "crates/cfwdon-worker/src/meta_placeholder_routes.rs::quote_owner_action_response",
            "crates/cfwdon-worker/src/remote/store.rs::upsert_remote_status_object",
        ],
        abstraction: "stored quote_state + quote-target resolution facts",
        operations: &[
            OperationMapping {
                model_action: "RemoteUpsert",
                domain_call: "merged_quote_state_for_remote_upsert",
                implementation_call: "remote/store.rs previous-row merge before upsert",
                worker_guard: "previous remote status row exists",
            },
            OperationMapping {
                model_action: "OwnerApprove",
                domain_call: "QuoteState::quote_state_after_owner_approve",
                implementation_call: "quote_owner_action_response(Approve)",
                worker_guard: "quote pending and quote_of_uri matches target",
            },
            OperationMapping {
                model_action: "OwnerReject",
                domain_call: "QuoteState::quote_state_after_owner_reject",
                implementation_call: "quote_owner_action_response(Reject)",
                worker_guard: "quote pending and quote_of_uri matches target",
            },
            OperationMapping {
                model_action: "Revoke",
                domain_call: "QuoteState::quote_state_after_revoke",
                implementation_call: "revoke_quote_response",
                worker_guard: "requester is quote author",
            },
        ],
    },
    RefinementEntry {
        model: "registration_transition_events",
        domain_module: "cfwdon_domain::account::registration",
        implementation_sites: &[
            "crates/cfwdon-worker/src/meta_placeholder_routes.rs::validate_account_registration_request",
        ],
        abstraction: "registration field presence + pipeline stage",
        operations: &[
            OperationMapping {
                model_action: "Validate",
                domain_call: "ComposingRegistration::validate",
                implementation_call: "validate_account_registration_request",
                worker_guard: "all required fields valid",
            },
            OperationMapping {
                model_action: "RegisterAndProvision",
                domain_call: "RegisteringAccount::provision",
                implementation_call: "account registration persistence path",
                worker_guard: "unique username/email",
            },
        ],
    },
    RefinementEntry {
        model: "status_draft_transition_events",
        domain_module: "cfwdon_domain::status::draft",
        implementation_sites: &[
            "crates/cfwdon-worker/src/statuses/request_parsing.rs",
            "crates/cfwdon-worker/src/statuses/mutations.rs::insert_status",
        ],
        abstraction: "composition payload shape + pipeline stage",
        operations: &[
            OperationMapping {
                model_action: "Validate",
                domain_call: "ComposingStatus::validate",
                implementation_call: "parse_status_draft_request",
                worker_guard: "payload passes domain validation",
            },
            OperationMapping {
                model_action: "ResolvePublishIntent",
                domain_call: "StatusDraft::into_publish_intent",
                implementation_call: "insert_status",
                worker_guard: "authenticated local publish",
            },
        ],
    },
    RefinementEntry {
        model: "inbox_replay",
        domain_module: "cfwdon_domain::federation::inbox",
        implementation_sites: &["(pending worker wiring)"],
        abstraction: "InboxActivityRecordState per activity id",
        operations: &[
            OperationMapping {
                model_action: "Receive",
                domain_call: "inbox_activity_after_receive",
                implementation_call: "(pending)",
                worker_guard: "first delivery only",
            },
            OperationMapping {
                model_action: "CompleteSuccess",
                domain_call: "inbox_activity_after_success",
                implementation_call: "(pending)",
                worker_guard: "in-flight row only",
            },
            OperationMapping {
                model_action: "CompleteFailure",
                domain_call: "inbox_activity_after_failure",
                implementation_call: "(pending)",
                worker_guard: "in-flight row only",
            },
        ],
    },
    RefinementEntry {
        model: "outbound_follow",
        domain_module: "cfwdon_domain::delivery",
        implementation_sites: &["crates/cfwdon-worker/src/delivery.rs"],
        abstraction: "outbound activity state + remote follow state",
        operations: &[],
    },
    RefinementEntry {
        model: "concurrent_delivery",
        domain_module: "cfwdon_domain::delivery",
        implementation_sites: &["crates/cfwdon-worker/src/delivery.rs"],
        abstraction: "two OutboundDeliverySlot values",
        operations: &[],
    },
    RefinementEntry {
        model: "outbox_delivery_pool",
        domain_module: "cfwdon_domain::delivery",
        implementation_sites: &["crates/cfwdon-worker/src/delivery.rs"],
        abstraction: "eight delivery attempt buckets",
        operations: &[],
    },
    RefinementEntry {
        model: "outbox_pipeline",
        domain_module: "cfwdon_domain::delivery",
        implementation_sites: &["crates/cfwdon-worker/src/delivery.rs"],
        abstraction: "generic outbox row + target expansion",
        operations: &[],
    },
    RefinementEntry {
        model: "local_follow_request",
        domain_module: "cfwdon_domain::follow",
        implementation_sites: &["crates/cfwdon-worker/src/follow_requests.rs"],
        abstraction: "local follow request + inbound remote request state",
        operations: &[],
    },
    RefinementEntry {
        model: "activitypub_visibility",
        domain_module: "cfwdon_domain::remote",
        implementation_sites: &["crates/cfwdon-worker/src/activitypub/objects.rs"],
        abstraction: "ActivityPub audience lists",
        operations: &[],
    },
    RefinementEntry {
        model: "status_draft_publish",
        domain_module: "cfwdon_domain::status::draft",
        implementation_sites: &[
            "crates/cfwdon-worker/src/statuses/request_parsing.rs",
            "crates/cfwdon-worker/src/statuses/mutations.rs",
        ],
        abstraction: "composition facts + publish stage",
        operations: &[],
    },
    RefinementEntry {
        model: "registration_pipeline",
        domain_module: "cfwdon_domain::account::registration",
        implementation_sites: &["crates/cfwdon-worker/src/meta_placeholder_routes.rs"],
        abstraction: "registration field inputs + pipeline stage",
        operations: &[],
    },
    RefinementEntry {
        model: "access_provision",
        domain_module: "cfwdon_domain::account::registration",
        implementation_sites: &["crates/cfwdon-worker/src/meta_placeholder_routes.rs"],
        abstraction: "OAuth email + username derivation",
        operations: &[],
    },
    RefinementEntry {
        model: "federation_request_policy",
        domain_module: "cfwdon_domain::federation",
        implementation_sites: &[
            "crates/cfwdon-worker/src/federation/request_validation.rs",
            "crates/cfwdon-worker/src/federation/url_guard.rs",
        ],
        abstraction: "signed-header facts + URL host + date skew",
        operations: &[],
    },
];
