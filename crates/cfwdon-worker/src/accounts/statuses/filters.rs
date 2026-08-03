use crate::{
    AccountStatusListOptions, AccountStatusVisibilityScope, AccountStatusesQuery,
    MediaAttachmentRow, RemoteAccountStatusListOptions, RemoteStatusRow, StatusRow,
    status_contains_tag,
};

pub(crate) fn remote_account_status_list_options<'a>(
    query: &'a AccountStatusesQuery,
    min_id: Option<&'a str>,
    limit: u32,
) -> RemoteAccountStatusListOptions<'a> {
    RemoteAccountStatusListOptions {
        max_id: query.max_id.as_deref(),
        min_id,
        limit,
        visibility: AccountStatusVisibilityScope::PublicUnlistedPrivate,
        only_media: query.only_media.unwrap_or(false),
        exclude_replies: query.exclude_replies.unwrap_or(false),
        exclude_reblogs: query.exclude_reblogs.unwrap_or(false),
        tagged: query.tagged.as_deref(),
    }
}

pub(crate) fn account_status_list_options<'a>(
    query: &'a AccountStatusesQuery,
    min_id: Option<&'a str>,
    limit: u32,
    visibility: AccountStatusVisibilityScope,
) -> AccountStatusListOptions<'a> {
    AccountStatusListOptions {
        max_id: query.max_id.as_deref(),
        min_id,
        limit,
        visibility,
        only_media: query.only_media.unwrap_or(false),
        exclude_replies: query.exclude_replies.unwrap_or(false),
        exclude_reblogs: query.exclude_reblogs.unwrap_or(false),
        tagged: query.tagged.as_deref(),
    }
}

pub(crate) fn local_status_matches_account_filters(
    status: &StatusRow,
    account_id: &str,
    query: &AccountStatusesQuery,
    media: &[MediaAttachmentRow],
    reply_account_id: Option<&String>,
) -> bool {
    if let Some(tag) = query.tagged.as_deref()
        && !status_contains_tag(status, tag)
    {
        return false;
    }
    if query.exclude_reblogs.unwrap_or(false) && status.boost_of_uri.is_some() {
        return false;
    }
    if query.exclude_replies.unwrap_or(false)
        && reply_account_id.is_some_and(|reply_account_id| reply_account_id != account_id)
    {
        return false;
    }
    !query.only_media.unwrap_or(false) || !media.is_empty()
}

pub(crate) fn remote_status_matches_account_filters(
    status: &RemoteStatusRow,
    query: &AccountStatusesQuery,
    has_media: bool,
) -> bool {
    if query.pinned.unwrap_or(false) {
        return false;
    }
    if let Some(tag) = query.tagged.as_deref()
        && !status
            .content_html
            .to_ascii_lowercase()
            .contains(&tag.to_ascii_lowercase())
    {
        return false;
    }
    if query.exclude_reblogs.unwrap_or(false) && status.boost_of_uri.is_some() {
        return false;
    }
    if query.exclude_replies.unwrap_or(false) && status.in_reply_to_uri.is_some() {
        return false;
    }
    !query.only_media.unwrap_or(false) || has_media
}

#[cfg(test)]
mod tests {
    use super::*;

    use cfwdon_domain::{QuoteState, Visibility};

    fn status_row() -> StatusRow {
        StatusRow {
            id: "status-1".to_owned(),
            account_id: "account-1".to_owned(),
            ap_id: None,
            in_reply_to_id: None,
            in_reply_to_account_id: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>#rust hello</p>".to_owned(),
            text: "#rust hello".to_owned(),
            spoiler_text: String::new(),
            visibility: Visibility::Public,
            sensitive: false,
            language: None,
            quote_approval_policy: None,
            quote_state: QuoteState::Accepted,
            application_id: None,
            card_json: None,
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: None,
        }
    }

    fn remote_status_row() -> RemoteStatusRow {
        RemoteStatusRow {
            id: "remote-1".to_owned(),
            actor_uri: "https://remote.example/users/alice".to_owned(),
            object_uri: "https://remote.example/users/alice/statuses/1".to_owned(),
            url: None,
            in_reply_to_uri: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>#rust hello</p>".to_owned(),
            text_content: "#rust hello".to_owned(),
            spoiler_text: String::new(),
            visibility: Visibility::Public,
            sensitive: false,
            language: None,
            quote_state: QuoteState::Accepted,
            published_at: "2026-01-01T00:00:00Z".to_owned(),
            edited_at: None,
            card_json: None,
            federated_emojis_json: "[]".to_owned(),
            in_reply_to_id: None,
        }
    }

    fn media_row() -> MediaAttachmentRow {
        MediaAttachmentRow {
            id: "media-1".to_owned(),
            account_id: "account-1".to_owned(),
            status_id: Some("status-1".to_owned()),
            object_key: "media/1".to_owned(),
            content_type: "image/png".to_owned(),
            description: String::new(),
            focus_x: None,
            focus_y: None,
            width: None,
            height: None,
            _created_at: "2026-01-01T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn local_status_account_filters_apply_tag_reply_reblog_and_media() {
        let mut status = status_row();
        let media = vec![media_row()];
        let query = AccountStatusesQuery {
            tagged: Some("rust".to_owned()),
            only_media: Some(true),
            ..Default::default()
        };
        assert!(local_status_matches_account_filters(
            &status,
            "account-1",
            &query,
            &media,
            None
        ));

        status.boost_of_uri = Some("https://example.com/boost".to_owned());
        let query = AccountStatusesQuery {
            exclude_reblogs: Some(true),
            ..Default::default()
        };
        assert!(!local_status_matches_account_filters(
            &status,
            "account-1",
            &query,
            &media,
            None
        ));

        status.boost_of_uri = None;
        status.in_reply_to_id = Some("reply-1".to_owned());
        let query = AccountStatusesQuery {
            exclude_replies: Some(true),
            ..Default::default()
        };
        assert!(!local_status_matches_account_filters(
            &status,
            "account-1",
            &query,
            &media,
            Some(&"account-2".to_owned())
        ));
    }

    #[test]
    fn remote_status_account_filters_apply_pinned_tag_reply_reblog_and_media() {
        let mut status = remote_status_row();
        let query = AccountStatusesQuery {
            tagged: Some("rust".to_owned()),
            only_media: Some(true),
            ..Default::default()
        };
        assert!(remote_status_matches_account_filters(&status, &query, true));

        let query = AccountStatusesQuery {
            pinned: Some(true),
            ..Default::default()
        };
        assert!(!remote_status_matches_account_filters(
            &status, &query, true
        ));

        status.in_reply_to_uri = Some("https://remote.example/statuses/root".to_owned());
        let query = AccountStatusesQuery {
            exclude_replies: Some(true),
            ..Default::default()
        };
        assert!(!remote_status_matches_account_filters(
            &status, &query, true
        ));
    }

    #[test]
    fn remote_account_status_list_options_reflect_query_flags() {
        let query = AccountStatusesQuery {
            max_id: Some("max".to_owned()),
            only_media: Some(true),
            exclude_replies: Some(true),
            exclude_reblogs: Some(true),
            tagged: Some("rust".to_owned()),
            ..Default::default()
        };

        let options = remote_account_status_list_options(&query, Some("min"), 24);

        assert_eq!(options.max_id, Some("max"));
        assert_eq!(options.min_id, Some("min"));
        assert_eq!(options.limit, 24);
        assert_eq!(
            options.visibility,
            AccountStatusVisibilityScope::PublicUnlistedPrivate
        );
        assert!(options.only_media);
        assert!(options.exclude_replies);
        assert!(options.exclude_reblogs);
        assert_eq!(options.tagged, Some("rust"));
    }

    #[test]
    fn local_account_status_list_options_reflect_query_flags() {
        let query = AccountStatusesQuery {
            max_id: Some("max".to_owned()),
            only_media: Some(true),
            exclude_replies: Some(true),
            exclude_reblogs: Some(true),
            tagged: Some("rust".to_owned()),
            ..Default::default()
        };

        let options =
            account_status_list_options(&query, Some("min"), 24, AccountStatusVisibilityScope::All);

        assert_eq!(options.max_id, Some("max"));
        assert_eq!(options.min_id, Some("min"));
        assert_eq!(options.limit, 24);
        assert_eq!(options.visibility, AccountStatusVisibilityScope::All);
        assert!(options.only_media);
        assert!(options.exclude_replies);
        assert!(options.exclude_reblogs);
        assert_eq!(options.tagged, Some("rust"));
    }
}
