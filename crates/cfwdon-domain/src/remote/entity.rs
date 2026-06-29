use crate::quote::QuoteState;
use crate::remote::record::RemoteStatusRecord;
use crate::status::Visibility;

/// Domain entity for a persisted remote status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteStatus {
    pub id: String,
    pub actor_uri: String,
    pub object_uri: String,
    pub url: Option<String>,
    pub in_reply_to_uri: Option<String>,
    pub boost_of_uri: Option<String>,
    pub quote_of_uri: Option<String>,
    pub content_html: String,
    pub spoiler_text: String,
    pub visibility: Visibility,
    pub sensitive: bool,
    pub language: Option<String>,
    pub quote_state: QuoteState,
    pub published_at: String,
}

impl RemoteStatus {
    pub fn from_record(record: RemoteStatusRecord) -> Self {
        Self {
            id: record.id,
            actor_uri: record.actor_uri,
            object_uri: record.object_uri,
            url: record.url,
            in_reply_to_uri: record.in_reply_to_uri,
            boost_of_uri: record.boost_of_uri,
            quote_of_uri: record.quote_of_uri,
            content_html: record.content_html,
            spoiler_text: record.spoiler_text,
            visibility: Visibility::parse(&record.visibility).unwrap_or(Visibility::Public),
            sensitive: record.sensitive != 0,
            language: record.language,
            quote_state: QuoteState::parse(&record.quote_state).unwrap_or(QuoteState::Accepted),
            published_at: record.published_at,
        }
    }

    pub fn to_record(&self) -> RemoteStatusRecord {
        RemoteStatusRecord {
            id: self.id.clone(),
            actor_uri: self.actor_uri.clone(),
            object_uri: self.object_uri.clone(),
            url: self.url.clone(),
            in_reply_to_uri: self.in_reply_to_uri.clone(),
            boost_of_uri: self.boost_of_uri.clone(),
            quote_of_uri: self.quote_of_uri.clone(),
            content_html: self.content_html.clone(),
            spoiler_text: self.spoiler_text.clone(),
            visibility: self.visibility.as_str().to_owned(),
            sensitive: i32::from(self.sensitive),
            language: self.language.clone(),
            quote_state: self.quote_state.as_str().to_owned(),
            published_at: self.published_at.clone(),
        }
    }

    pub fn effective_quote_state(&self) -> QuoteState {
        QuoteState::effective_for_stored(self.quote_of_uri.as_deref(), self.quote_state)
    }

    pub fn has_active_quote(&self) -> bool {
        self.quote_of_uri.is_some() && self.effective_quote_state().is_visible()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_status_record_roundtrip_preserves_entity() {
        let status = RemoteStatus {
            id: "remote-1".to_owned(),
            actor_uri: "https://remote.example/users/bob".to_owned(),
            object_uri: "https://remote.example/users/bob/statuses/1".to_owned(),
            url: None,
            in_reply_to_uri: None,
            boost_of_uri: None,
            quote_of_uri: Some("https://example.com/status/2".to_owned()),
            content_html: "<p>hello</p>".to_owned(),
            spoiler_text: String::new(),
            visibility: Visibility::Unlisted,
            sensitive: true,
            language: Some("en".to_owned()),
            quote_state: QuoteState::Pending,
            published_at: "2026-01-01T00:00:00Z".to_owned(),
        };

        let restored = RemoteStatus::from_record(status.to_record());
        assert_eq!(status, restored);
    }
}
