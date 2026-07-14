use std::collections::HashSet;
use std::hash::Hash;

use stateright::Model;

/// Walk every reachable abstract state and assert the model's `next_state` matches
/// the pure domain transition function.
pub(crate) fn assert_model_matches_domain<M, F>(model: &M, domain_step: F)
where
    M: Model,
    M::State: Copy + Eq + Hash + std::fmt::Debug,
    M::Action: Copy + Eq + Hash + std::fmt::Debug,
    F: Fn(M::State, M::Action) -> Option<M::State>,
{
    let mut seen = HashSet::new();
    let mut stack = model.init_states();
    while let Some(state) = stack.pop() {
        if !seen.insert(state) {
            continue;
        }

        let mut actions = Vec::new();
        model.actions(&state, &mut actions);
        for action in actions {
            let model_next = model.next_state(&state, action);
            let domain_next = domain_step(state, action);
            assert_eq!(
                model_next, domain_next,
                "model/domain step mismatch for state={state:?} action={action:?}"
            );
            if let Some(next) = model_next {
                stack.push(next);
            }
        }
    }
}

/// When the worker guard allows an operation, its effect must match the domain step.
/// When the guard rejects, the observable state must stutter.
pub(crate) fn assert_worker_refinement<S, A, G, E, F>(
    initial_states: impl IntoIterator<Item = S>,
    mut actions: impl FnMut(&S) -> Vec<A>,
    mut worker_allows: G,
    mut worker_effect: E,
    mut domain_step: F,
) where
    S: Copy + Eq + Hash + std::fmt::Debug,
    A: Copy + Eq + Hash + std::fmt::Debug,
    G: FnMut(S, A) -> bool,
    E: FnMut(S, A) -> S,
    F: FnMut(S, A) -> S,
{
    let mut seen = HashSet::new();
    let mut stack: Vec<S> = initial_states.into_iter().collect();
    while let Some(state) = stack.pop() {
        if !seen.insert(state) {
            continue;
        }

        for action in actions(&state) {
            if worker_allows(state, action) {
                let worker_next = worker_effect(state, action);
                let domain_next = domain_step(state, action);
                assert_eq!(
                    worker_next, domain_next,
                    "worker/domain mismatch for state={state:?} action={action:?}"
                );
                stack.push(worker_next);
            } else {
                assert_eq!(
                    worker_effect(state, action),
                    state,
                    "worker must stutter when guard rejects state={state:?} action={action:?}"
                );
            }
        }
    }
}
