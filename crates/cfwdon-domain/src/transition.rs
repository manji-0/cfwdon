/// Outcome of a domain state transition that may emit side-effect events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition<TState, TEvent> {
    pub state: TState,
    pub events: Vec<TEvent>,
}

impl<TState, TEvent> Transition<TState, TEvent> {
    pub fn without_events(state: TState) -> Self {
        Self {
            state,
            events: Vec::new(),
        }
    }

    pub fn with_event(state: TState, event: TEvent) -> Self {
        Self {
            state,
            events: vec![event],
        }
    }
}
