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
                implementation_call: "clear_local_status_quote / clear_remote_status_quote",
                worker_guard: "requester is quote author",
            },
        ],
    },
    RefinementEntry {
        model: "registration_transition_events",
        domain_module: "cfwdon_domain::account::registration",
        implementation_sites: &[
            "crates/cfwdon-worker/src/meta_placeholder_routes.rs::account_registration_api_details",
        ],
        abstraction: "registration field presence + pipeline stage",
        operations: &[
            OperationMapping {
                model_action: "Validate",
                domain_call: "ComposingRegistration::validate",
                implementation_call: "account_registration_api_details",
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
        implementation_sites: &[
            "crates/cfwdon-worker/src/inbox/activity_store.rs",
            "crates/cfwdon-worker/src/inbox.rs",
        ],
        abstraction: "InboxActivityRecordState per activity id",
        operations: &[
            OperationMapping {
                model_action: "Receive",
                domain_call: "inbox_activity_after_receive",
                implementation_call: "begin_inbox_activity_processing",
                worker_guard: "INSERT OR IGNORE returns a row",
            },
            OperationMapping {
                model_action: "CompleteSuccess",
                domain_call: "inbox_activity_after_success",
                implementation_call: "mark_inbox_activity_processed",
                worker_guard: "in-flight row only",
            },
            OperationMapping {
                model_action: "CompleteFailure",
                domain_call: "inbox_activity_after_failure",
                implementation_call: "release_inbox_activity_processing",
                worker_guard: "in-flight row with processed_at IS NULL",
            },
        ],
    },
    RefinementEntry {
        model: "outbound_follow",
        domain_module: "cfwdon_domain::delivery",
        implementation_sites: &[
            "crates/cfwdon-worker/src/delivery.rs",
            "crates/cfwdon-worker/src/delivery/outbound_state.rs",
            "crates/cfwdon-worker/src/inbox/follow_handlers.rs",
        ],
        abstraction: "outbound activity state + remote follow state",
        operations: &[
            OperationMapping {
                model_action: "DeliverySucceeds",
                domain_call: "outbound_state_after_delivery_attempt",
                implementation_call: "mark_outbound_activity_delivered",
                worker_guard: "outbound row queued",
            },
            OperationMapping {
                model_action: "DeliveryFails",
                domain_call: "reconcile_pending_follow_on_outbound_terminal_failure",
                implementation_call: "reconcile_outbound_activity_terminal_failure",
                worker_guard: "outbound row queued",
            },
            OperationMapping {
                model_action: "ReceiveFollowAccept",
                domain_call: "follow_state_after_inbox_response",
                implementation_call: "update_follow_state_from_response(Accept)",
                worker_guard: "activity is Follow response",
            },
            OperationMapping {
                model_action: "ReceiveFollowReject",
                domain_call: "follow_state_after_inbox_response",
                implementation_call: "update_follow_state_from_response(Reject)",
                worker_guard: "activity is Follow response",
            },
        ],
    },
    RefinementEntry {
        model: "concurrent_delivery",
        domain_module: "cfwdon_domain::delivery",
        implementation_sites: &["crates/cfwdon-worker/src/delivery.rs"],
        abstraction: "two independent OutboundDeliverySlot values",
        operations: &[
            OperationMapping {
                model_action: "SucceedSlot0 / SucceedSlot1",
                domain_call: "outbound_delivery_slot_after_attempt",
                implementation_call: "mark_*_delivered",
                worker_guard: "slot row still queued",
            },
            OperationMapping {
                model_action: "FailSlot0 / FailSlot1",
                domain_call: "outbound_delivery_slot_after_attempt",
                implementation_call: "reschedule_* or reconcile_*_terminal_failure",
                worker_guard: "slot row still queued",
            },
        ],
    },
    RefinementEntry {
        model: "outbox_delivery_pool",
        domain_module: "cfwdon_domain::delivery",
        implementation_sites: &["crates/cfwdon-worker/src/delivery.rs"],
        abstraction: "eight OutboundDeliverySlot buckets by attempt_count",
        operations: &[
            OperationMapping {
                model_action: "SucceedAt",
                domain_call: "outbound_delivery_slot_after_attempt",
                implementation_call: "mark_outbox_delivery_delivered / mark_outbound_activity_delivered",
                worker_guard: "queued row at attempt index",
            },
            OperationMapping {
                model_action: "FailAt",
                domain_call: "outbound_delivery_slot_after_attempt",
                implementation_call: "reschedule_* or reconcile_*_terminal_failure",
                worker_guard: "queued row at attempt index",
            },
        ],
    },
    RefinementEntry {
        model: "outbox_pipeline",
        domain_module: "cfwdon_domain::delivery",
        implementation_sites: &["crates/cfwdon-worker/src/delivery.rs"],
        abstraction: "generic outbox parent row + expanded target slots",
        operations: &[
            OperationMapping {
                model_action: "ExpandGeneric",
                domain_call: "generic_outbox_parent_state_after_expand",
                implementation_call: "partition_generic_outbox_deliveries_by_targets + mark_*_expanded",
                worker_guard: "generic parent row queued",
            },
            OperationMapping {
                model_action: "Target0Succeeds / Target1Succeeds",
                domain_call: "outbox_delivery_state_after_attempt",
                implementation_call: "mark_outbox_delivery_delivered",
                worker_guard: "active target row queued",
            },
            OperationMapping {
                model_action: "Target0Fails / Target1Fails",
                domain_call: "outbox_delivery_state_after_attempt",
                implementation_call: "reschedule_outbox_delivery or mark_outbox_delivery_terminal_failure",
                worker_guard: "active target row queued",
            },
        ],
    },
    RefinementEntry {
        model: "local_follow_request",
        domain_module: "cfwdon_domain::follow",
        implementation_sites: &["crates/cfwdon-worker/src/follow_requests.rs"],
        abstraction: "local follow request + inbound remote request state",
        operations: &[
            OperationMapping {
                model_action: "Authorize",
                domain_call: "authorize_local_follow_request",
                implementation_call: "authorize_pending_follow_request",
                worker_guard: "pending local follow or queued remote request",
            },
            OperationMapping {
                model_action: "Reject",
                domain_call: "reject_local_follow_request",
                implementation_call: "reject_pending_follow_request",
                worker_guard: "pending local follow or queued remote request",
            },
        ],
    },
    RefinementEntry {
        model: "activitypub_visibility",
        domain_module: "cfwdon_domain::remote::activitypub",
        implementation_sites: &[
            "crates/cfwdon-worker/src/activitypub/objects.rs",
            "crates/cfwdon-worker/src/activitypub/parse.rs",
        ],
        abstraction: "ActivityPub to/cc public audience flags",
        operations: &[
            OperationMapping {
                model_action: "EmitPublic / EmitUnlisted / EmitFollowersOnly / EmitDirect",
                domain_call: "activitypub_audience_flags_for_visibility",
                implementation_call: "activitypub_audiences",
                worker_guard: "local note emission",
            },
            OperationMapping {
                model_action: "ToggleToPublic / ToggleCcPublic",
                domain_call: "visibility_from_activitypub_audiences",
                implementation_call: "visibility_from_activitypub_object",
                worker_guard: "remote object ingest",
            },
        ],
    },
    RefinementEntry {
        model: "status_draft_publish",
        domain_module: "cfwdon_domain::status::draft",
        implementation_sites: &[
            "crates/cfwdon-worker/src/statuses/request_parsing.rs",
            "crates/cfwdon-worker/src/statuses/mutations.rs",
        ],
        abstraction: "composition facts + publish stage",
        operations: &[
            OperationMapping {
                model_action: "Validate",
                domain_call: "ComposingStatus::validate",
                implementation_call: "parse_status_draft",
                worker_guard: "composition submitted for publish",
            },
            OperationMapping {
                model_action: "Publish",
                domain_call: "StatusDraft::into_publish_intent",
                implementation_call: "insert_status",
                worker_guard: "draft validated successfully",
            },
        ],
    },
    RefinementEntry {
        model: "registration_pipeline",
        domain_module: "cfwdon_domain::account::registration",
        implementation_sites: &["crates/cfwdon-worker/src/meta_placeholder_routes.rs"],
        abstraction: "registration field inputs + pipeline stage",
        operations: &[
            OperationMapping {
                model_action: "Validate",
                domain_call: "ComposingRegistration::validate",
                implementation_call: "account_registration_api_details",
                worker_guard: "registration form submitted",
            },
            OperationMapping {
                model_action: "Register",
                domain_call: "finalize_registration_validation",
                implementation_call: "insert_registered_account",
                worker_guard: "validated registration and unique username/email",
            },
            OperationMapping {
                model_action: "Provision",
                domain_call: "RegisteredAccount::provision",
                implementation_call: "store_account_password / issue_oauth_access_token",
                worker_guard: "account row created",
            },
        ],
    },
    RefinementEntry {
        model: "access_provision",
        domain_module: "cfwdon_domain::account::registration",
        implementation_sites: &["crates/cfwdon-worker/src/auth/account_store.rs"],
        abstraction: "OAuth email + username derivation",
        operations: &[
            OperationMapping {
                model_action: "Resolve",
                domain_call: "ComposingAccessProvision::resolve",
                implementation_call: "resolve_local_account",
                worker_guard: "authenticated user email not yet provisioned",
            },
            OperationMapping {
                model_action: "Register",
                domain_call: "AccessProvisionIntent::register",
                implementation_call: "INSERT INTO accounts",
                worker_guard: "resolved username and email",
            },
            OperationMapping {
                model_action: "Provision",
                domain_call: "RegisteringAccount::provision",
                implementation_call: "store_account_private_key",
                worker_guard: "account row inserted",
            },
        ],
    },
    RefinementEntry {
        model: "federation_dns_policy",
        domain_module: "cfwdon_domain::federation::dns",
        implementation_sites: &["crates/cfwdon-worker/src/federation/url_guard.rs"],
        abstraction: "static host policy + DNS A/AAAA answers + validation cache",
        operations: &[
            OperationMapping {
                model_action: "CycleDnsResolution",
                domain_call: "remote_hostname_dns_resolution_allowed",
                implementation_call: "validate_remote_hostname_resolution",
                worker_guard: "hostname is not an IP literal and cache miss",
            },
            OperationMapping {
                model_action: "ToggleCacheHit",
                domain_call: "remote_url_policy_from_parts",
                implementation_call: "remote_hostname_validation_cache_hit",
                worker_guard: "prior successful DNS validation within TTL",
            },
        ],
    },
    RefinementEntry {
        model: "federation_request_policy",
        domain_module: "cfwdon_domain::federation",
        implementation_sites: &[
            "crates/cfwdon-worker/src/http/request_validation.rs",
            "crates/cfwdon-worker/src/federation/url_guard.rs",
        ],
        abstraction: "signed-header facts + URL host + date skew",
        operations: &[
            OperationMapping {
                model_action: "CycleSignedHeaders",
                domain_call: "activitypub_signature_lists_required_headers",
                implementation_call: "validate_activitypub_signature_headers",
                worker_guard: "parsed Signature header present",
            },
            OperationMapping {
                model_action: "CycleKeyIdCase",
                domain_call: "activitypub_key_id_matches_actor",
                implementation_call: "key_id_matches_actor",
                worker_guard: "activity actor URI resolved",
            },
            OperationMapping {
                model_action: "ToggleCachedPublicKeyId",
                domain_call: "cached_remote_actor_key_matches",
                implementation_call: "cached_remote_actor_matches_key",
                worker_guard: "remote actor profile loaded",
            },
            OperationMapping {
                model_action: "CycleRemoteHost",
                domain_call: "remote_url_policy_from_parts",
                implementation_call: "parse_remote_http_url + url_guard",
                worker_guard: "outbound federation fetch",
            },
            OperationMapping {
                model_action: "ToggleDateSkew",
                domain_call: "activitypub_date_within_skew",
                implementation_call: "validate_request_date",
                worker_guard: "Date header parses",
            },
        ],
    },
];
