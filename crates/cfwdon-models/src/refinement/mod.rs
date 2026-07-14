mod activitypub_visibility;
mod catalog;
mod concurrent_delivery;
mod federation_dns;
mod federation_request_policy;
mod inbox_replay;
mod local_follow_request;
mod outbound_follow;
mod outbox_delivery_pool;
mod outbox_pipeline;
mod quote;
mod quote_approval;
mod registration;
mod status_draft;
pub(crate) mod verify;

pub use catalog::{OperationMapping, REFINEMENT_CATALOG, RefinementEntry};

pub(crate) fn verify_refinements() {
    quote_approval::check_quote_approval_refinement();
    registration::check_registration_refinement();
    status_draft::check_status_draft_refinement();
    federation_dns::check_federation_dns_refinement();
    inbox_replay::check_inbox_replay_refinement();
    federation_request_policy::check_federation_request_policy_refinement();
    outbox_delivery_pool::check_outbox_delivery_pool_refinement();
    concurrent_delivery::check_concurrent_delivery_refinement();
    outbox_pipeline::check_outbox_pipeline_refinement();
    outbound_follow::check_outbound_follow_refinement();
    local_follow_request::check_local_follow_request_refinement();
    activitypub_visibility::check_activitypub_visibility_refinement();
    quote::check_quote_refinement();
}

#[cfg(test)]
mod tests {
    use super::REFINEMENT_CATALOG;

    #[test]
    fn catalog_lists_every_model() {
        let names: Vec<_> = REFINEMENT_CATALOG.iter().map(|entry| entry.model).collect();
        assert!(names.contains(&"quote"));
        assert!(names.contains(&"quote_approval"));
        assert!(names.contains(&"status_draft_transition_events"));
        assert!(names.contains(&"federation_dns_policy"));
        assert_eq!(names.len(), 16);
    }
}
