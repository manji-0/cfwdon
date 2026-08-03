use crate::error::RecordHydrationError;
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
    pub text_content: String,
    pub spoiler_text: String,
    pub visibility: Visibility,
    pub sensitive: bool,
    pub language: Option<String>,
    pub quote_state: QuoteState,
    pub published_at: String,
    pub edited_at: Option<String>,
    pub card_json: Option<String>,
    pub federated_emojis_json: String,
    pub in_reply_to_id: Option<String>,
}

impl RemoteStatus {
    pub fn try_from_record(record: RemoteStatusRecord) -> Result<Self, RecordHydrationError> {
        Ok(Self {
            id: record.id,
            actor_uri: record.actor_uri,
            object_uri: record.object_uri,
            url: record.url,
            in_reply_to_uri: record.in_reply_to_uri,
            boost_of_uri: record.boost_of_uri,
            quote_of_uri: record.quote_of_uri,
            content_html: record.content_html,
            text_content: record.text_content,
            spoiler_text: record.spoiler_text,
            visibility: Visibility::parse(&record.visibility)?,
            sensitive: record.sensitive != 0,
            language: record.language,
            quote_state: QuoteState::parse(&record.quote_state)?,
            published_at: record.published_at,
            edited_at: record.edited_at,
            card_json: record.card_json,
            federated_emojis_json: if record.federated_emojis_json.is_empty() {
                "[]".to_owned()
            } else {
                record.federated_emojis_json
            },
            in_reply_to_id: record.in_reply_to_id,
        })
    }

    pub fn from_record(record: RemoteStatusRecord) -> Self {
        Self::try_from_record(record).expect("valid remote status record")
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
            text_content: self.text_content.clone(),
            spoiler_text: self.spoiler_text.clone(),
            visibility: self.visibility.as_str().to_owned(),
            sensitive: i32::from(self.sensitive),
            language: self.language.clone(),
            quote_state: self.quote_state.as_str().to_owned(),
            published_at: self.published_at.clone(),
            edited_at: self.edited_at.clone(),
            card_json: self.card_json.clone(),
            federated_emojis_json: self.federated_emojis_json.clone(),
            in_reply_to_id: self.in_reply_to_id.clone(),
        }
    }

    pub fn effective_quote_state(&self) -> QuoteState {
        QuoteState::effective_for_stored(self.quote_of_uri.as_deref(), self.quote_state)
    }

    pub fn has_active_quote(&self) -> bool {
        self.quote_of_uri.is_some() && self.effective_quote_state().is_visible()
    }

    /// Plain-text body for API responses. Prefers the write-time column and
    /// falls back to stripping HTML for rows written before that column existed.
    pub fn plain_text(&self) -> String {
        if !self.text_content.is_empty() {
            self.text_content.clone()
        } else {
            strip_basic_html_tags(&self.content_html)
        }
    }
}

fn strip_basic_html_tags(html: &str) -> String {
    let mut output = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
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
            text_content: "hello".to_owned(),
            spoiler_text: String::new(),
            visibility: Visibility::Unlisted,
            sensitive: true,
            language: Some("en".to_owned()),
            quote_state: QuoteState::Pending,
            published_at: "2026-01-01T00:00:00Z".to_owned(),
            edited_at: None,
            card_json: None,
            federated_emojis_json: "[]".to_owned(),
            in_reply_to_id: None,
        };

        let restored = RemoteStatus::from_record(status.to_record());
        assert_eq!(status, restored);
    }
}
