mod catalog;
mod federation_dns;
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
