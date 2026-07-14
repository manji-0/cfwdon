use cfwdon_domain::{QuoteApprovalPolicy, QuoteState, QuoteTargetResolution, Visibility};

use crate::quote::{QuoteAction, QuoteModel, QuoteModelState, apply_quote_action};
use crate::refinement::verify::{assert_model_matches_domain, assert_worker_refinement};
use stateright::Model;

const ACCOUNT_DEFAULT_POLICY: QuoteApprovalPolicy = QuoteApprovalPolicy::Followers;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct QuoteWorkerContext {
    state: QuoteModelState,
    requester_is_quote_author: bool,
}

fn model_domain_step(state: QuoteModelState, action: QuoteAction) -> Option<QuoteModelState> {
    QuoteModel.next_state(&state, action)
}

/// Mirrors `QuoteTargetResolution::initial_state` / `into_publish_intent`.
fn worker_resolve_initial(state: QuoteModelState) -> QuoteState {
    QuoteTargetResolution {
        has_quote: state.has_quote,
        target_exists_locally: state.target_exists_locally,
    }
    .initial_state()
}

/// Mirrors `StatusDraft::effective_quote_policy` with no explicit override.
fn worker_apply_visibility_policy(visibility: Visibility) -> QuoteApprovalPolicy {
    QuoteApprovalPolicy::for_status_visibility(visibility, None, ACCOUNT_DEFAULT_POLICY)
}

/// Mirrors `QuoteState::remote_for_target` in the worker.
fn worker_resolve_remote(state: QuoteModelState) -> QuoteState {
    let policy_allows = state.policy.allows_quote(state.is_owner, state.is_follower);
    QuoteState::remote_for_target(state.blocked_by_owner, policy_allows)
}

fn worker_allows(context: QuoteWorkerContext, action: QuoteAction) -> bool {
    match action {
        QuoteAction::ResolveInitial
        | QuoteAction::ResolveRemote
        | QuoteAction::ApplyVisibilityPolicy => true,
        QuoteAction::Revoke => context.requester_is_quote_author && context.state.has_quote,
    }
}

fn worker_effect(mut context: QuoteWorkerContext, action: QuoteAction) -> QuoteWorkerContext {
    if !worker_allows(context, action) {
        return context;
    }

    match action {
        QuoteAction::ResolveInitial => {
            context.state.quote_state = worker_resolve_initial(context.state);
        }
        QuoteAction::ResolveRemote => {
            context.state.quote_state = worker_resolve_remote(context.state);
        }
        QuoteAction::ApplyVisibilityPolicy => {
            context.state.policy = worker_apply_visibility_policy(context.state.visibility);
        }
        QuoteAction::Revoke => {
            apply_quote_action(&mut context.state, action);
        }
    }

    context
}

fn domain_step(mut context: QuoteWorkerContext, action: QuoteAction) -> QuoteWorkerContext {
    context.state = model_domain_step(context.state, action).unwrap_or(context.state);
    context
}

fn worker_contexts() -> Vec<QuoteWorkerContext> {
    let mut contexts = Vec::new();
    for state in QuoteModel.init_states() {
        for requester_is_quote_author in [false, true] {
            contexts.push(QuoteWorkerContext {
                state,
                requester_is_quote_author,
            });
        }
    }
    contexts
}

pub(crate) fn check_quote_refinement() {
    assert_model_matches_domain(&QuoteModel, model_domain_step);

    assert_worker_refinement(
        worker_contexts(),
        |_| {
            vec![
                QuoteAction::ResolveInitial,
                QuoteAction::ResolveRemote,
                QuoteAction::ApplyVisibilityPolicy,
                QuoteAction::Revoke,
            ]
        },
        worker_allows,
        worker_effect,
        domain_step,
    );

    for visibility in [
        Visibility::Public,
        Visibility::Unlisted,
        Visibility::FollowersOnly,
        Visibility::Direct,
    ] {
        assert_eq!(
            worker_apply_visibility_policy(visibility),
            QuoteApprovalPolicy::for_status_visibility(visibility, None, ACCOUNT_DEFAULT_POLICY),
            "visibility policy for {visibility:?}"
        );
    }

    let resolution = QuoteTargetResolution::with_target(false);
    assert_eq!(
        worker_resolve_initial(QuoteModelState {
            quote_state: QuoteState::Pending,
            visibility: Visibility::Public,
            policy: QuoteApprovalPolicy::Public,
            has_quote: true,
            target_exists_locally: false,
            blocked_by_owner: false,
            is_owner: false,
            is_follower: false,
        }),
        resolution.initial_state()
    );
}

#[cfg(test)]
mod tests {
    use super::check_quote_refinement;

    #[test]
    fn quote_refinement_holds() {
        check_quote_refinement();
    }
}
