//! Model-checking harnesses for `cfwdon` domain protocols.
//!
//! Each module implements a [`stateright::Model`] that delegates transition logic to
//! pure functions in [`cfwdon_domain`], so the checker explores outcomes without
//! duplicating business rules.
//!
//! [`refinement`] maps each model to worker call sites and checks that guarded
//! implementation steps refine the abstract transitions.

#![allow(dead_code)]

pub mod refinement;

mod access_provision;
mod activitypub_visibility;
mod concurrent_delivery;
mod federation_request_policy;
mod inbox_replay;
mod local_follow_request;
mod outbound_follow;
mod outbox_delivery_pool;
mod outbox_pipeline;
mod quote;
mod quote_approval;
mod registration_pipeline;
mod registration_transition_events;
mod status_draft_publish;
mod status_draft_transition_events;

/// Run every Stateright model in this crate. Panics when a property is violated.
pub fn verify_models() {
    quote::check_quote_model();
    quote_approval::check_quote_approval_model();
    local_follow_request::check_local_follow_request_model();
    outbound_follow::check_outbound_follow_model();
    concurrent_delivery::check_concurrent_delivery_model();
    outbox_delivery_pool::check_outbox_delivery_pool_model();
    outbox_pipeline::check_outbox_pipeline_model();
    activitypub_visibility::check_activitypub_visibility_model();
    status_draft_publish::check_status_draft_publish_model();
    status_draft_transition_events::check_status_draft_transition_events_model();
    registration_pipeline::check_registration_pipeline_model();
    registration_transition_events::check_registration_transition_events_model();
    access_provision::check_access_provision_model();
    inbox_replay::check_inbox_replay_model();
    federation_request_policy::check_federation_request_policy_model();
    refinement::verify_refinements();
}
