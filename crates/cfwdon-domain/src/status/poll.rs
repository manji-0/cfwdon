use crate::error::PollDraftError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct PollDraft {
    options: Vec<String>,
    expires_in_seconds: u64,
    multiple: bool,
    hide_totals: bool,
}

impl PollDraft {
    pub fn try_new(
        options: Vec<String>,
        expires_in_seconds: u64,
        multiple: bool,
        hide_totals: bool,
    ) -> Result<Self, PollDraftError> {
        let options = options
            .into_iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if options.len() < 2 || options.len() > 4 {
            return Err(PollDraftError::InvalidOptionCount);
        }
        if expires_in_seconds < 300 {
            return Err(PollDraftError::ExpiresInTooShort);
        }
        Ok(Self {
            options,
            expires_in_seconds,
            multiple,
            hide_totals,
        })
    }

    pub fn options(&self) -> &[String] {
        &self.options
    }

    pub fn expires_in_seconds(&self) -> u64 {
        self.expires_in_seconds
    }

    pub fn multiple(&self) -> bool {
        self.multiple
    }

    pub fn hide_totals(&self) -> bool {
        self.hide_totals
    }
}

/// Persistence-ready local poll vote before D1 insert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredLocalPollVoteIntent {
    pub vote_id: String,
    pub poll_id: String,
    pub account_id: String,
    pub option_position: u32,
    pub activity_uri: Option<String>,
}

impl StoredLocalPollVoteIntent {
    pub fn new(
        vote_id: impl Into<String>,
        poll_id: impl Into<String>,
        account_id: impl Into<String>,
        option_position: u32,
        activity_uri: Option<String>,
    ) -> Self {
        Self {
            vote_id: vote_id.into(),
            poll_id: poll_id.into(),
            account_id: account_id.into(),
            option_position,
            activity_uri,
        }
    }
}

/// Persistence-ready remote poll vote before D1 insert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredRemotePollVoteIntent {
    pub vote_id: String,
    pub poll_id: String,
    pub account_id: String,
    pub option_position: u32,
    pub option_title: String,
    pub activity_id: String,
}

impl StoredRemotePollVoteIntent {
    pub fn new(
        vote_id: impl Into<String>,
        poll_id: impl Into<String>,
        account_id: impl Into<String>,
        option_position: u32,
        option_title: impl Into<String>,
        activity_id: impl Into<String>,
    ) -> Self {
        Self {
            vote_id: vote_id.into(),
            poll_id: poll_id.into(),
            account_id: account_id.into(),
            option_position,
            option_title: option_title.into(),
            activity_id: activity_id.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_draft_rejects_too_few_options() {
        let error = PollDraft::try_new(vec!["only".to_owned()], 300, false, false).unwrap_err();
        assert_eq!(error, PollDraftError::InvalidOptionCount);
    }
}
