//! Model-checking harnesses for `cfwdon` domain protocols.
//!
//! Each module implements a [`stateright::Model`] that delegates transition logic to
//! pure functions in [`cfwdon_domain`], so the checker explores outcomes without
//! duplicating business rules.

#![allow(dead_code)]

mod concurrent_delivery;
mod outbound_follow;
mod quote;

/// Run every Stateright model in this crate. Panics when a property is violated.
pub fn verify_models() {
    quote::check_quote_model();
    outbound_follow::check_outbound_follow_model();
    concurrent_delivery::check_concurrent_delivery_model();
}
