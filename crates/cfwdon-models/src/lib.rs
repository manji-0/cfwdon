//! Model-checking harnesses for `cfwdon` domain protocols.
//!
//! Each module implements a [`stateright::Model`] that delegates transition logic to
//! pure functions in [`cfwdon_domain`], so the checker explores outcomes without
//! duplicating business rules.

#![allow(dead_code)]

mod activitypub_visibility;
mod concurrent_delivery;
mod local_follow_request;
mod outbound_follow;
mod outbox_pipeline;
mod quote;
mod quote_approval;

/// Run every Stateright model in this crate. Panics when a property is violated.
pub fn verify_models() {
    quote::check_quote_model();
    quote_approval::check_quote_approval_model();
    local_follow_request::check_local_follow_request_model();
    outbound_follow::check_outbound_follow_model();
    concurrent_delivery::check_concurrent_delivery_model();
    outbox_pipeline::check_outbox_pipeline_model();
    activitypub_visibility::check_activitypub_visibility_model();
}
