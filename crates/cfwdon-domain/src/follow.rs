//! Pure local follow and inbound remote follow-request rules.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LocalFollowState {
    Pending,
    Accepted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FollowRequestScenario {
    LocalFollower,
    RemoteFollower,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RemoteInboundFollowRequestState {
    Absent,
    Queued,
    Fulfilled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LocalFollowRequestState {
    pub scenario: FollowRequestScenario,
    pub target_locked: bool,
    pub local_follow: Option<LocalFollowState>,
    pub remote_request: RemoteInboundFollowRequestState,
}

pub fn initial_local_follow_state(target_locked: bool) -> LocalFollowState {
    if target_locked {
        LocalFollowState::Pending
    } else {
        LocalFollowState::Accepted
    }
}

pub fn local_follow_state_after_authorize(current: LocalFollowState) -> LocalFollowState {
    match current {
        LocalFollowState::Pending => LocalFollowState::Accepted,
        LocalFollowState::Accepted => LocalFollowState::Accepted,
    }
}

pub fn local_follow_exists_after_reject(current: LocalFollowState) -> bool {
    !matches!(current, LocalFollowState::Pending)
}

pub fn local_follow_notification_type(state: LocalFollowState) -> &'static str {
    match state {
        LocalFollowState::Pending => "follow_request",
        LocalFollowState::Accepted => "follow",
    }
}

pub fn remote_inbound_request_after_inbox_follow(
    target_locked: bool,
) -> RemoteInboundFollowRequestState {
    if target_locked {
        RemoteInboundFollowRequestState::Queued
    } else {
        RemoteInboundFollowRequestState::Fulfilled
    }
}

pub fn remote_inbound_request_after_authorize(
    current: RemoteInboundFollowRequestState,
) -> RemoteInboundFollowRequestState {
    match current {
        RemoteInboundFollowRequestState::Queued => RemoteInboundFollowRequestState::Fulfilled,
        other => other,
    }
}

pub fn remote_inbound_request_after_reject(
    current: RemoteInboundFollowRequestState,
) -> RemoteInboundFollowRequestState {
    match current {
        RemoteInboundFollowRequestState::Queued => RemoteInboundFollowRequestState::Absent,
        other => other,
    }
}

pub fn initial_local_follow_request_state(
    scenario: FollowRequestScenario,
    target_locked: bool,
) -> LocalFollowRequestState {
    match scenario {
        FollowRequestScenario::LocalFollower => LocalFollowRequestState {
            scenario,
            target_locked,
            local_follow: Some(initial_local_follow_state(target_locked)),
            remote_request: RemoteInboundFollowRequestState::Absent,
        },
        FollowRequestScenario::RemoteFollower => {
            let remote_request = remote_inbound_request_after_inbox_follow(target_locked);
            LocalFollowRequestState {
                scenario,
                target_locked,
                local_follow: if remote_request == RemoteInboundFollowRequestState::Fulfilled {
                    Some(LocalFollowState::Accepted)
                } else {
                    None
                },
                remote_request,
            }
        }
    }
}

pub fn authorize_local_follow_request(
    mut state: LocalFollowRequestState,
) -> LocalFollowRequestState {
    if let Some(local_follow) = state.local_follow {
        state.local_follow = Some(local_follow_state_after_authorize(local_follow));
    }
    state.remote_request = remote_inbound_request_after_authorize(state.remote_request);
    if state.remote_request == RemoteInboundFollowRequestState::Fulfilled
        && state.local_follow.is_none()
    {
        state.local_follow = Some(LocalFollowState::Accepted);
    }
    state
}

pub fn reject_local_follow_request(mut state: LocalFollowRequestState) -> LocalFollowRequestState {
    if let Some(local_follow) = state.local_follow {
        state.local_follow = if local_follow_exists_after_reject(local_follow) {
            Some(local_follow)
        } else {
            None
        };
    }
    state.remote_request = remote_inbound_request_after_reject(state.remote_request);
    state
}

impl LocalFollowState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_local_target_starts_pending_for_local_follower() {
        let state = initial_local_follow_request_state(FollowRequestScenario::LocalFollower, true);
        assert_eq!(state.local_follow, Some(LocalFollowState::Pending));
        assert_eq!(
            state.remote_request,
            RemoteInboundFollowRequestState::Absent
        );
    }

    #[test]
    fn locked_remote_follower_queues_request_before_follow_row_exists() {
        let state = initial_local_follow_request_state(FollowRequestScenario::RemoteFollower, true);
        assert_eq!(state.local_follow, None);
        assert_eq!(
            state.remote_request,
            RemoteInboundFollowRequestState::Queued
        );
    }

    #[test]
    fn authorize_moves_pending_local_follow_to_accepted() {
        let initial =
            initial_local_follow_request_state(FollowRequestScenario::LocalFollower, true);
        let authorized = authorize_local_follow_request(initial);
        assert_eq!(authorized.local_follow, Some(LocalFollowState::Accepted));
    }

    #[test]
    fn reject_deletes_pending_local_follow_row() {
        let initial =
            initial_local_follow_request_state(FollowRequestScenario::LocalFollower, true);
        let rejected = reject_local_follow_request(initial);
        assert_eq!(rejected.local_follow, None);
    }
}
