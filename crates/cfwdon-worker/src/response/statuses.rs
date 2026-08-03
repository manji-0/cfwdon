use crate::{
    AppConfig, FederatedEmojiMap, LocalAccount, MastodonMediaAttachmentResponse,
    MastodonStatusResponse, MastodonStatusTagResponse, MediaAttachmentRow, RemoteActorRow,
    RemoteStatusRow, StatusRow, actor_url, custom_emojis_used_in_texts, extract_hashtags_from_html,
    extract_hashtags_from_text, resolve_status_emojis, tag_url, timestamp_to_mastodon_iso8601,
    timestamp_to_mastodon_iso8601_opt,
};

pub(crate) struct LocalStatusResponseDetails {
    pub(crate) application: Option<serde_json::Value>,
    pub(crate) card: Option<serde_json::Value>,
    pub(crate) poll: Option<serde_json::Value>,
    pub(crate) mentions: Vec<serde_json::Value>,
    pub(crate) favourites_count: u64,
    pub(crate) reblogs_count: u64,
    pub(crate) quotes_count: u64,
    pub(crate) favourited: Option<bool>,
    pub(crate) reblogged: Option<bool>,
    pub(crate) muted: Option<bool>,
    pub(crate) bookmarked: Option<bool>,
    pub(crate) pinned: Option<bool>,
    pub(crate) edited_at: Option<String>,
    pub(crate) filtered: Option<Vec<serde_json::Value>>,
    pub(crate) quote_approval: Option<serde_json::Value>,
    pub(crate) quote: Option<serde_json::Value>,
}

pub(crate) struct RemoteStatusResponseDetails {
    pub(crate) media_attachments: Vec<serde_json::Value>,
    pub(crate) card: Option<serde_json::Value>,
    pub(crate) poll: Option<serde_json::Value>,
    pub(crate) mentions: Vec<serde_json::Value>,
    pub(crate) favourites_count: u64,
    pub(crate) reblogs_count: u64,
    pub(crate) quotes_count: u64,
    pub(crate) favourited: Option<bool>,
    pub(crate) reblogged: Option<bool>,
    pub(crate) muted: Option<bool>,
    pub(crate) bookmarked: Option<bool>,
    pub(crate) in_reply_to_id: Option<String>,
    pub(crate) edited_at: Option<String>,
    pub(crate) filtered: Option<Vec<serde_json::Value>>,
    pub(crate) quote_approval: Option<serde_json::Value>,
    pub(crate) quote: Option<serde_json::Value>,
}

fn status_tag_values(
    config: &AppConfig,
    tags: impl IntoIterator<Item = String>,
) -> Vec<serde_json::Value> {
    tags.into_iter()
        .map(|tag| {
            serde_json::to_value(MastodonStatusTagResponse {
                name: tag.clone(),
                url: tag_url(config, &tag),
            })
            .unwrap_or(serde_json::Value::Null)
        })
        .collect()
}

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
                actor_url(config, account.username()),
                row.id
            )
        });

        Self {
            id: row.id.clone(),
            created_at: timestamp_to_mastodon_iso8601(&row.created_at),
            in_reply_to_id: row.in_reply_to_id.clone(),
            in_reply_to_account_id,
            sensitive: row.sensitive,
            spoiler_text: row.spoiler_text.clone(),
            visibility: row.visibility.as_str().to_owned(),
            language: row.language.clone(),
            uri: uri.clone(),
            url: uri,
            replies_count: 0,
            reblogs_count: 0,
            favourites_count: 0,
            quotes_count: 0,
            favourited: None,
            reblogged: None,
            muted: None,
            bookmarked: None,
            pinned: None,
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
            tags: status_tag_values(config, extract_hashtags_from_text(&row.text)),
            emojis: custom_emojis_used_in_texts(
                [row.text.as_str(), row.spoiler_text.as_str()],
                config,
            ),
            quote_approval: None,
            card: None,
            poll: None,
            edited_at: None,
            filtered: None,
            quote: None,
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
        response.content = String::new();
        response.text = Some(row.text.clone());
        response
    }

    pub(crate) fn from_remote_row(
        row: &RemoteStatusRow,
        actor: &RemoteActorRow,
        config: &AppConfig,
        federated_emojis: Option<&FederatedEmojiMap>,
    ) -> Self {
        let uri = row.object_uri.clone();
        let url = row.url.clone().unwrap_or_else(|| uri.clone());

        Self {
            id: row.id.clone(),
            created_at: timestamp_to_mastodon_iso8601(&row.published_at),
            in_reply_to_id: None,
            in_reply_to_account_id: None,
            sensitive: row.sensitive,
            spoiler_text: row.spoiler_text.clone(),
            visibility: row.visibility.as_str().to_owned(),
            language: row.language.clone(),
            uri,
            url,
            replies_count: 0,
            reblogs_count: 0,
            favourites_count: 0,
            quotes_count: 0,
            favourited: None,
            reblogged: None,
            muted: None,
            bookmarked: None,
            pinned: None,
            content: row.content_html.clone(),
            text: None,
            reblog: None,
            application: None,
            account: crate::MastodonAccountResponse::from_remote_actor(actor),
            media_attachments: Vec::new(),
            mentions: Vec::new(),
            tags: status_tag_values(config, extract_hashtags_from_html(&row.content_html)),
            emojis: resolve_status_emojis(
                federated_emojis,
                &[row.plain_text().as_str(), row.spoiler_text.as_str()],
                config,
            ),
            quote_approval: None,
            card: None,
            poll: None,
            edited_at: None,
            filtered: None,
            quote: None,
        }
    }

    pub(crate) fn apply_local_details(&mut self, details: LocalStatusResponseDetails) {
        self.application = details.application;
        self.card = details.card;
        self.poll = details.poll;
        self.mentions = details.mentions;
        self.favourites_count = details.favourites_count;
        self.reblogs_count = details.reblogs_count;
        self.quotes_count = details.quotes_count;
        self.favourited = details.favourited;
        self.reblogged = details.reblogged;
        self.muted = details.muted;
        self.bookmarked = details.bookmarked;
        self.pinned = details.pinned;
        self.edited_at = timestamp_to_mastodon_iso8601_opt(details.edited_at.as_deref());
        self.filtered = details.filtered;
        self.quote_approval = details.quote_approval;
        self.quote = details.quote;
    }

    pub(crate) fn apply_remote_details(&mut self, details: RemoteStatusResponseDetails) {
        self.media_attachments = details.media_attachments;
        self.card = details.card;
        self.poll = details.poll;
        self.mentions = details.mentions;
        self.favourites_count = details.favourites_count;
        self.reblogs_count = details.reblogs_count;
        self.quotes_count = details.quotes_count;
        self.favourited = details.favourited;
        self.reblogged = details.reblogged;
        self.muted = details.muted;
        self.bookmarked = details.bookmarked;
        self.in_reply_to_id = details.in_reply_to_id;
        self.edited_at = timestamp_to_mastodon_iso8601_opt(details.edited_at.as_deref());
        self.filtered = details.filtered;
        self.quote_approval = details.quote_approval;
        self.quote = details.quote;
    }
}
