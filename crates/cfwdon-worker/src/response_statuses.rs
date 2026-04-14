use crate::{
    AppConfig, LocalAccount, MastodonMediaAttachmentResponse, MastodonStatusResponse,
    MastodonTagResponse, MediaAttachmentRow, RemoteActorRow, RemoteStatusRow, StatusRow, actor_url,
    extract_hashtags_from_html, extract_hashtags_from_text, tag_history_stub, tag_rest_id, tag_url,
};

impl MastodonStatusResponse {
    pub(crate) fn from_row(
        row: &StatusRow,
        account: &LocalAccount,
        config: &AppConfig,
        in_reply_to_account_id: Option<String>,
        media_attachments: Vec<MediaAttachmentRow>,
    ) -> Self {
        let uri = row.ap_id.clone().unwrap_or_else(|| {
            format!(
                "{}/statuses/{}",
                actor_url(config, &account.username),
                row.id
            )
        });

        Self {
            id: row.id.clone(),
            created_at: row.created_at.clone(),
            in_reply_to_id: row.in_reply_to_id.clone(),
            in_reply_to_account_id,
            sensitive: row.sensitive != 0,
            spoiler_text: row.spoiler_text.clone(),
            visibility: row.visibility.clone(),
            language: row.language.clone(),
            uri: uri.clone(),
            url: uri,
            replies_count: 0,
            reblogs_count: 0,
            favourites_count: 0,
            favourited: false,
            reblogged: false,
            muted: false,
            bookmarked: false,
            pinned: false,
            content: row.content_html.clone(),
            text: None,
            reblog: None,
            application: None,
            account: crate::MastodonAccountResponse::from_account(account, config),
            media_attachments: media_attachments
                .iter()
                .map(|media| {
                    serde_json::to_value(MastodonMediaAttachmentResponse::from_row(media, config))
                        .unwrap_or(serde_json::Value::Null)
                })
                .collect(),
            mentions: Vec::new(),
            tags: extract_hashtags_from_text(&row._text_content)
                .into_iter()
                .map(|tag| {
                    serde_json::to_value(MastodonTagResponse {
                        id: tag_rest_id(&tag),
                        name: tag.clone(),
                        url: tag_url(config, &tag),
                        history: tag_history_stub(),
                        following: false,
                        featured: false,
                    })
                    .unwrap_or(serde_json::Value::Null)
                })
                .collect(),
            emojis: Vec::new(),
            card: None,
            poll: None,
        }
    }

    pub(crate) fn from_deleted_row(
        row: &StatusRow,
        account: &LocalAccount,
        config: &AppConfig,
        in_reply_to_account_id: Option<String>,
        media_attachments: Vec<MediaAttachmentRow>,
    ) -> Self {
        let mut response = Self::from_row(
            row,
            account,
            config,
            in_reply_to_account_id,
            media_attachments,
        );
        response.text = Some(row._text_content.clone());
        response
    }

    pub(crate) fn from_remote_row(
        row: &RemoteStatusRow,
        actor: &RemoteActorRow,
        config: &AppConfig,
    ) -> Self {
        let uri = row.object_uri.clone();
        let url = row.url.clone().unwrap_or_else(|| uri.clone());

        Self {
            id: row.id.clone(),
            created_at: row.published_at.clone(),
            in_reply_to_id: row.in_reply_to_uri.clone(),
            in_reply_to_account_id: None,
            sensitive: row.sensitive != 0,
            spoiler_text: row.spoiler_text.clone(),
            visibility: row.visibility.clone(),
            language: row.language.clone(),
            uri,
            url,
            replies_count: 0,
            reblogs_count: 0,
            favourites_count: 0,
            favourited: false,
            reblogged: false,
            muted: false,
            bookmarked: false,
            pinned: false,
            content: row.content_html.clone(),
            text: None,
            reblog: None,
            application: None,
            account: crate::MastodonAccountResponse::from_remote_actor(actor),
            media_attachments: Vec::new(),
            mentions: Vec::new(),
            tags: extract_hashtags_from_html(&row.content_html)
                .into_iter()
                .map(|tag| {
                    serde_json::to_value(MastodonTagResponse {
                        id: tag_rest_id(&tag),
                        name: tag.clone(),
                        url: tag_url(config, &tag),
                        history: tag_history_stub(),
                        following: false,
                        featured: false,
                    })
                    .unwrap_or(serde_json::Value::Null)
                })
                .collect(),
            emojis: Vec::new(),
            card: None,
            poll: None,
        }
    }
}
