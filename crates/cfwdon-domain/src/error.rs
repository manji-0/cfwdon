use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdError {
    #[error("identifier must not be empty")]
    Empty,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum QuoteApprovalPolicyError {
    #[error("quote_approval_policy must be one of: public, followers, nobody")]
    Unknown,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum QuoteStateError {
    #[error("quote_state must be one of: accepted, pending, rejected, revoked")]
    Unknown,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VisibilityError {
    #[error("visibility must be one of: public, unlisted, private, direct")]
    Unknown,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PollDraftError {
    #[error("poll must include between 2 and 4 non-empty options")]
    InvalidOptionCount,
    #[error("poll expires_in must be at least 300 seconds")]
    ExpiresInTooShort,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RecordHydrationError {
    #[error(transparent)]
    Visibility(#[from] VisibilityError),
    #[error(transparent)]
    QuoteState(#[from] QuoteStateError),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RemoteStatusError {
    #[error("remote status object is missing id")]
    MissingObjectId,
    #[error("remote announce activity is missing object id")]
    MissingBoostTarget,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StatusDraftError {
    #[error("status, media_ids, or poll must be present")]
    EmptyPayload,
    #[error("a maximum of 4 media attachments is supported")]
    TooManyMedia,
    #[error("poll cannot be combined with media attachments yet")]
    PollWithMedia,
    #[error("quoted statuses cannot be combined with media attachments or polls")]
    QuoteWithMediaOrPoll,
    #[error(transparent)]
    Poll(#[from] PollDraftError),
    #[error(transparent)]
    QuotePolicy(#[from] QuoteApprovalPolicyError),
}
