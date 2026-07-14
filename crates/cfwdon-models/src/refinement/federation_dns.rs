use stateright::Model;

use crate::federation_dns_policy::{
    FederationDnsPolicyAction, FederationDnsPolicyModel, FederationDnsPolicyModelState,
};
use crate::refinement::verify::assert_model_matches_domain;

fn model_domain_step(
    state: FederationDnsPolicyModelState,
    action: FederationDnsPolicyAction,
) -> Option<FederationDnsPolicyModelState> {
    FederationDnsPolicyModel.next_state(&state, action)
}

pub(crate) fn check_federation_dns_refinement() {
    assert_model_matches_domain(&FederationDnsPolicyModel, model_domain_step);
}

#[cfg(test)]
mod tests {
    use super::check_federation_dns_refinement;

    #[test]
    fn federation_dns_refinement_holds() {
        check_federation_dns_refinement();
    }
}
