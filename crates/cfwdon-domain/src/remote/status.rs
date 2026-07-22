use crate::error::RemoteStatusError;
use crate::ids::StatusId;
use crate::quote::QuoteState;
use crate::remote::activitypub::{
    audience_values_contain_followers, audience_values_contains_public,
    quote_target_uri_from_fields, visibility_from_activitypub_audiences,
};
use crate::status::Visibility;
use crate::transition::Transition;

/// ActivityPub Note fields normalized at the worker boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityPubStatusInput {
    pub object_id: Option<String>,
    pub url: Option<String>,
    pub in_reply_to: Option<String>,
    pub quote_uri: Option<String>,
    pub quote_url: Option<String>,
    pub misskey_quote: Option<String>,
    pub content_html: String,
    pub spoiler_text: Option<String>,
    pub to_audiences: Vec<String>,
    pub cc_audiences: Vec<String>,
    pub sensitive: Option<bool>,
    pub language: Option<String>,
    pub published_at: Option<String>,
    pub updated_at: Option<String>,
}

impl ActivityPubStatusInput {
    pub fn into_incoming(self) -> Result<IncomingRemoteStatus, RemoteStatusError> {
        let object_uri = self
            .object_id
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .ok_or(RemoteStatusError::MissingObjectId)?;
        let quote_of_uri = quote_target_uri_from_fields(
            self.quote_uri.as_deref(),
            self.quote_url.as_deref(),
            self.misskey_quote.as_deref(),
        );
        let visibility = visibility_from_activitypub_audiences(
            audience_values_contains_public(&self.to_audiences),
            audience_values_contains_public(&self.cc_audiences),
            audience_values_contain_followers(&self.to_audiences)
                || audience_values_contain_followers(&self.cc_audiences),
        );
        let published_at = self
            .published_at
            .or(self.updated_at)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_default();

        Ok(IncomingRemoteStatus {
            object_uri,
            url: normalize_optional_uri(self.url),
            in_reply_to_uri: normalize_optional_uri(self.in_reply_to),
            quote_of_uri,
            content_html: self.content_html,
            spoiler_text: self
                .spoiler_text
                .map(|value| value.trim().to_owned())
                .unwrap_or_default(),
            visibility,
            sensitive: self.sensitive.unwrap_or(false),
            language: normalize_optional_language(self.language),
            published_at,
        })
    }
}

/// Parsed remote status ready for quote resolution and persistence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingRemoteStatus {
    object_uri: String,
    url: Option<String>,
    in_reply_to_uri: Option<String>,
    quote_of_uri: Option<String>,
    content_html: String,
    spoiler_text: String,
    visibility: Visibility,
    sensitive: bool,
    language: Option<String>,
    published_at: String,
}

impl IncomingRemoteStatus {
    pub fn object_uri(&self) -> &str {
        &self.object_uri
    }

    pub fn quote_of_uri(&self) -> Option<&str> {
        self.quote_of_uri.as_deref()
    }

    pub fn visibility(&self) -> Visibility {
        self.visibility
    }

    pub fn published_at(&self) -> &str {
        &self.published_at
    }

    pub fn language(&self) -> &Option<String> {
        &self.language
    }

    pub fn into_store_intent(
        self,
        status_id: StatusId,
        actor_uri: String,
        quote_resolution: RemoteQuoteResolution,
        raw_object_json: String,
        revision_at: String,
    ) -> Transition<StoredRemoteStatusIntent, ()> {
        Transition::without_events(StoredRemoteStatusIntent {
            status_id,
            actor_uri,
            object_uri: self.object_uri,
            url: self.url,
            in_reply_to_uri: self.in_reply_to_uri,
            quote_of_uri: self.quote_of_uri,
            content_html: self.content_html,
            spoiler_text: self.spoiler_text,
            visibility: self.visibility,
            sensitive: self.sensitive,
            language: self.language,
            quote_state: quote_resolution.initial_quote_state(),
            published_at: self.published_at,
            raw_object_json,
            revision_at,
        })
    }
}

/// Facts about a quoted local status resolved outside the domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteQuoteLocalTarget {
    pub blocked_by_owner: bool,
    pub policy_allows: bool,
}

/// Quote metadata resolved before storing a remote status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteQuoteResolution {
    quote_of_uri: Option<String>,
    local_target: Option<RemoteQuoteLocalTarget>,
}

impl RemoteQuoteResolution {
    pub fn without_quote() -> Self {
        Self {
            quote_of_uri: None,
            local_target: None,
        }
    }

    pub fn accepted_quote(quote_of_uri: String) -> Self {
        Self {
            quote_of_uri: Some(quote_of_uri),
            local_target: None,
        }
    }

    pub fn with_local_target(quote_of_uri: String, local_target: RemoteQuoteLocalTarget) -> Self {
        Self {
            quote_of_uri: Some(quote_of_uri),
            local_target: Some(local_target),
        }
    }

    pub fn initial_quote_state(&self) -> QuoteState {
        match (&self.quote_of_uri, &self.local_target) {
            (None, _) => QuoteState::Accepted,
            (Some(_), None) => QuoteState::Accepted,
            (Some(_), Some(target)) => {
                QuoteState::remote_for_target(target.blocked_by_owner, target.policy_allows)
            }
        }
    }
}

/// Persistence-ready remote status after quote resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredRemoteStatusIntent {
    pub status_id: StatusId,
    pub actor_uri: String,
    pub object_uri: String,
    pub url: Option<String>,
    pub in_reply_to_uri: Option<String>,
    pub quote_of_uri: Option<String>,
    pub content_html: String,
    pub spoiler_text: String,
    pub visibility: Visibility,
    pub sensitive: bool,
    pub language: Option<String>,
    pub quote_state: QuoteState,
    pub published_at: String,
    pub raw_object_json: String,
    pub revision_at: String,
}

impl StoredRemoteStatusIntent {
    pub fn effective_quote_state(&self) -> QuoteState {
        QuoteState::effective_for_stored(self.quote_of_uri.as_deref(), self.quote_state)
    }

    pub fn has_active_quote(&self) -> bool {
        self.quote_of_uri.is_some() && self.effective_quote_state().is_visible()
    }
}

/// ActivityPub Announce fields normalized at the worker boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityPubReblogInput {
    pub activity_id: Option<String>,
    pub boost_of_uri: Option<String>,
    pub quote_uri: Option<String>,
    pub quote_url: Option<String>,
    pub misskey_quote: Option<String>,
    pub to_audiences: Vec<String>,
    pub cc_audiences: Vec<String>,
    pub published_at: Option<String>,
    pub updated_at: Option<String>,
}

impl ActivityPubReblogInput {
    pub fn into_incoming(self) -> Result<IncomingRemoteReblog, RemoteStatusError> {
        let object_uri = self
            .activity_id
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .ok_or(RemoteStatusError::MissingObjectId)?;
        let boost_of_uri = self
            .boost_of_uri
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .ok_or(RemoteStatusError::MissingBoostTarget)?;
        let quote_of_uri = quote_target_uri_from_fields(
            self.quote_uri.as_deref(),
            self.quote_url.as_deref(),
            self.misskey_quote.as_deref(),
        );
        let visibility = visibility_from_activitypub_audiences(
            audience_values_contains_public(&self.to_audiences),
            audience_values_contains_public(&self.cc_audiences),
            audience_values_contain_followers(&self.to_audiences)
                || audience_values_contain_followers(&self.cc_audiences),
        );
        let published_at = self
            .published_at
            .or(self.updated_at)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_default();

        Ok(IncomingRemoteReblog {
            object_uri,
            boost_of_uri,
            quote_of_uri,
            visibility,
            published_at,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingRemoteReblog {
    object_uri: String,
    boost_of_uri: String,
    quote_of_uri: Option<String>,
    visibility: Visibility,
    published_at: String,
}

impl IncomingRemoteReblog {
    pub fn into_store_intent(
        self,
        status_id: StatusId,
        actor_uri: String,
        quote_resolution: RemoteQuoteResolution,
        raw_object_json: String,
    ) -> Transition<StoredRemoteReblogIntent, ()> {
        Transition::without_events(StoredRemoteReblogIntent {
            status_id,
            actor_uri,
            object_uri: self.object_uri,
            boost_of_uri: self.boost_of_uri,
            quote_of_uri: self.quote_of_uri,
            visibility: self.visibility,
            quote_state: quote_resolution.initial_quote_state(),
            published_at: self.published_at,
            raw_object_json,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredRemoteReblogIntent {
    pub status_id: StatusId,
    pub actor_uri: String,
    pub object_uri: String,
    pub boost_of_uri: String,
    pub quote_of_uri: Option<String>,
    pub visibility: Visibility,
    pub quote_state: QuoteState,
    pub published_at: String,
    pub raw_object_json: String,
}

fn normalize_optional_uri(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalize_optional_language(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::StatusId;

    #[test]
    fn incoming_remote_status_builds_store_intent_with_quote_state() {
        let incoming = ActivityPubStatusInput {
            object_id: Some("https://remote.example/statuses/1".to_owned()),
            url: None,
            in_reply_to: None,
            quote_uri: Some("https://local.example/statuses/2".to_owned()),
            quote_url: None,
            misskey_quote: None,
            content_html: "<p>hi</p>".to_owned(),
            spoiler_text: None,
            to_audiences: vec![crate::remote::activitypub::ACTIVITYSTREAMS_PUBLIC.to_owned()],
            cc_audiences: Vec::new(),
            sensitive: None,
            language: None,
            published_at: Some("2026-01-01T00:00:00Z".to_owned()),
            updated_at: None,
        }
        .into_incoming()
        .expect("incoming status");

        let intent = incoming
            .into_store_intent(
                StatusId::new("remote-1").expect("status id"),
                "https://remote.example/users/alice".to_owned(),
                RemoteQuoteResolution::with_local_target(
                    "https://local.example/statuses/2".to_owned(),
                    RemoteQuoteLocalTarget {
                        blocked_by_owner: false,
                        policy_allows: false,
                    },
                ),
                "{}".to_owned(),
                "revision".to_owned(),
            )
            .state;

        assert_eq!(intent.quote_state, QuoteState::Pending);
        assert_eq!(intent.visibility, Visibility::Public);
    }
}
