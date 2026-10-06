mod draft;
mod local;
mod poll;
mod record;
mod stored;
mod visibility;

pub use draft::{
    ComposingStatus, PublishIntent, QuoteTargetResolution, StatusDraft, StatusDraftEvent,
};
pub use local::LocalStatus;
pub use poll::{PollDraft, StoredLocalPollVoteIntent, StoredRemotePollVoteIntent};
pub use record::{LocalStatusRecord, local_status_default_quote_state};
pub use stored::{
    LocalReblogPersistenceFacts, LocalStatusPersistenceFacts, StoredLocalReblogIntent,
    StoredLocalStatusIntent,
};
pub use visibility::Visibility;

/// Denormalized per-status counters kept in `status_counts` /
/// `remote_status_counts` by triggers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatusInteractionCounts {
    pub favourites: u64,
    pub reblogs: u64,
    pub replies: u64,
}
