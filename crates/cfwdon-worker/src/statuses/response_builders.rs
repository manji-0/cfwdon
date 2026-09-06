//! Status API response builders.
//!
//! Local and remote status response entry points in this module are intentional
//! graph bridges: timelines, quotes, and detail routes converge here so shared
//! preload/viewer/quote embedding stays consistent. Child modules split local,
//! remote, quote embed, reblog orchestration, and filter helpers; prefer
//! extending those seams rather than adding route-specific forks here.

mod filtered;
mod local;
mod quote_embed;
mod reblog;
mod remote;

#[allow(unused_imports)]
pub(crate) use local::{
    build_loaded_local_status_response, build_local_status_response,
    build_local_status_response_with_filter_matcher, build_local_status_response_with_preloads,
    build_local_status_response_with_quote_count_preloads,
    build_local_status_response_with_timeline_preloads,
};
#[allow(unused_imports)]
pub(crate) use remote::{
    build_remote_status_response, build_remote_status_response_with_filter_matcher,
    build_remote_status_response_with_preloads,
    build_remote_status_response_with_timeline_preloads,
};

#[cfg(test)]
mod tests {
    use super::super::reblog_response::{
        local_reblog_wrapper_response_from_embedded, remote_reblog_wrapper_response_from_embedded,
    };
    use super::super::{
        AppConfig, LocalAccount, MastodonStatusResponse, RemoteActorRow, RemoteStatusRow, StatusRow,
    };
    use super::remote::remote_media_attachment_values;
    use cfwdon_domain::LocalAccountRecord;

    #[test]
    fn remote_media_attachment_values_allows_empty_attachments() {
        assert!(remote_media_attachment_values(&[]).is_empty());
    }

    #[test]
    fn remote_reblog_wrapper_response_overlays_wrapper_fields_and_clears_embedded_body() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let wrapper_actor = remote_actor_row_fixture();
        let wrapper_status =
            remote_status_row_fixture("wrapper-status", "https://remote.example/announce/1");
        let embedded_status =
            remote_status_row_fixture("embedded-status", "https://remote.example/statuses/1");
        let mut embedded = MastodonStatusResponse::from_remote_row(
            &embedded_status,
            &wrapper_actor,
            &config,
            None,
        );
        embedded.media_attachments = vec![serde_json::json!({"id": "media-1"})];
        embedded.quote = Some(serde_json::json!({"state": "accepted"}));

        let response = remote_reblog_wrapper_response_from_embedded(
            Some(embedded),
            &wrapper_status,
            &wrapper_actor,
            &config,
        );

        assert_eq!(response.id, "wrapper-status");
        assert_eq!(response.uri, "https://remote.example/announce/1");
        assert!(response.reblog.is_some());
        assert!(response.content.is_empty());
        assert!(response.media_attachments.is_empty());
        assert!(response.quote.is_none());
    }

    #[test]
    fn local_reblog_wrapper_response_overlays_wrapper_fields_and_clears_embedded_body() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let wrapper_account = local_account_fixture();
        let wrapper_status = status_row_fixture(
            "wrapper-status",
            Some("https://social.example/users/alice/statuses/wrapper"),
        );
        let embedded_status = status_row_fixture(
            "embedded-status",
            Some("https://social.example/users/alice/statuses/embedded"),
        );
        let mut embedded = MastodonStatusResponse::from_row(
            &embedded_status,
            &wrapper_account,
            &config,
            None,
            Vec::new(),
        );
        embedded.media_attachments = vec![serde_json::json!({"id": "media-1"})];
        embedded.quote = Some(serde_json::json!({"state": "accepted"}));

        let response = local_reblog_wrapper_response_from_embedded(
            Some(embedded),
            &wrapper_status,
            &wrapper_account,
            Some("reply-account".to_owned()),
            &config,
        );

        assert_eq!(response.id, "wrapper-status");
        assert_eq!(
            response.uri,
            "https://social.example/users/alice/statuses/wrapper"
        );
        assert_eq!(
            response.in_reply_to_account_id.as_deref(),
            Some("reply-account")
        );
        assert!(response.reblog.is_some());
        assert!(response.content.is_empty());
        assert!(response.media_attachments.is_empty());
        assert!(response.quote.is_none());
    }

    fn remote_status_row_fixture(id: &str, object_uri: &str) -> RemoteStatusRow {
        RemoteStatusRow {
            id: id.to_owned(),
            actor_uri: "https://remote.example/users/alice".to_owned(),
            object_uri: object_uri.to_owned(),
            url: None,
            in_reply_to_uri: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>Hello</p>".to_owned(),
            text_content: "Hello".to_owned(),
            spoiler_text: String::new(),
            visibility: cfwdon_domain::Visibility::Public,
            sensitive: false,
            language: Some("en".to_owned()),
            quote_state: cfwdon_domain::QuoteState::Accepted,
            published_at: "2026-05-10T01:02:03Z".to_owned(),
            edited_at: None,
            card_json: None,
            federated_emojis_json: "[]".to_owned(),
            in_reply_to_id: None,
            interaction_counts: None,
        }
    }

    fn remote_actor_row_fixture() -> RemoteActorRow {
        RemoteActorRow {
            actor_uri: "https://remote.example/users/alice".to_owned(),
            username: "alice".to_owned(),
            domain: "remote.example".to_owned(),
            created_at: "2026-05-01T00:00:00Z".to_owned(),
            locked: false,
            bot: false,
            discoverable: true,
            indexable: true,
            display_name: "Alice".to_owned(),
            summary_html: String::new(),
            profile_url: Some("https://remote.example/@alice".to_owned()),
            avatar_url: None,
            header_url: None,
            followers_count: 0,
            following_count: 0,
            statuses_count: 0,
            social_counts_updated_at: None,
        }
    }

    fn status_row_fixture(id: &str, ap_id: Option<&str>) -> StatusRow {
        StatusRow {
            id: id.to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: ap_id.map(str::to_owned),
            in_reply_to_id: None,
            in_reply_to_account_id: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>Hello</p>".to_owned(),
            text: "Hello".to_owned(),
            spoiler_text: String::new(),
            visibility: cfwdon_domain::Visibility::Public,
            sensitive: false,
            language: Some("en".to_owned()),
            quote_approval_policy: None,
            quote_state: cfwdon_domain::QuoteState::Accepted,
            application_id: None,
            card_json: None,
            created_at: "2026-05-10T01:02:03Z".to_owned(),
            updated_at: None,
        }
    }

    fn local_account_fixture() -> LocalAccount {
        LocalAccount::from_record(LocalAccountRecord::test_fixture("acct-1", "alice"))
    }
}
