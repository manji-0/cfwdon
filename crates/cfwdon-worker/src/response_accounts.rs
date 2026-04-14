use crate::{
    AccountStats, AppConfig, LocalAccount, MastodonAccountResponse, MastodonAccountSource,
    ProfileField, RemoteActorRow, actor_url, escape_html, media_object_url, remote_account_rest_id,
};
use url::Url;

pub(crate) fn mastodon_account_fields(fields: &[ProfileField]) -> Vec<serde_json::Value> {
    fields
        .iter()
        .map(|field| {
            serde_json::json!({
                "name": field.name,
                "value": render_profile_field_value_html(&field.value),
                "verified_at": serde_json::Value::Null,
            })
        })
        .collect()
}

pub(crate) fn render_profile_field_value_html(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if Url::parse(trimmed)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .is_some()
    {
        let escaped = escape_html(trimmed);
        return format!(
            "<a href=\"{escaped}\" rel=\"nofollow noopener noreferrer me\" target=\"_blank\">{escaped}</a>"
        );
    }
    escape_html(trimmed)
}

fn account_avatar_url(config: &AppConfig, account: &LocalAccount) -> String {
    account
        .avatar_object_key
        .as_deref()
        .map(|object_key| media_object_url(config, object_key))
        .unwrap_or_default()
}

fn account_header_url(config: &AppConfig, account: &LocalAccount) -> String {
    account
        .header_object_key
        .as_deref()
        .map(|object_key| media_object_url(config, object_key))
        .unwrap_or_default()
}

impl MastodonAccountResponse {
    pub(crate) fn from_account(account: &LocalAccount, config: &AppConfig) -> Self {
        Self::from_account_with_stats(account, config, &AccountStats::default())
    }

    pub(crate) fn from_account_with_stats(
        account: &LocalAccount,
        config: &AppConfig,
        stats: &AccountStats,
    ) -> Self {
        let profile_url = actor_url(config, &account.username);

        Self {
            id: account.id.clone(),
            username: account.username.clone(),
            acct: account.acct().to_owned(),
            display_name: account.display_name.clone(),
            locked: false,
            bot: false,
            created_at: account.created_at.clone(),
            note: account.bio_html.clone(),
            url: profile_url,
            avatar: account_avatar_url(config, account),
            avatar_static: account_avatar_url(config, account),
            header: account_header_url(config, account),
            header_static: account_header_url(config, account),
            fields: mastodon_account_fields(&account.fields),
            followers_count: stats.followers_count,
            following_count: stats.following_count,
            statuses_count: stats.statuses_count,
            source: None,
        }
    }

    pub(crate) fn from_credentials_account(
        account: &LocalAccount,
        config: &AppConfig,
        stats: &AccountStats,
    ) -> Self {
        let mut value = Self::from_account_with_stats(account, config, stats);
        value.source = Some(MastodonAccountSource {
            note: account.bio_text.clone(),
            fields: mastodon_account_fields(&account.fields),
            privacy: account.default_post_visibility.clone(),
            sensitive: account.default_sensitive,
            language: account.default_language.clone().unwrap_or_default(),
            follow_requests_count: 0,
            hide_collections: None,
            discoverable: Some(account.discoverable),
        });
        value
    }

    pub(crate) fn from_remote_actor(actor: &RemoteActorRow) -> Self {
        let profile_url = actor
            .profile_url
            .clone()
            .unwrap_or_else(|| actor.actor_uri.clone());
        let avatar_url = actor.avatar_url.clone().unwrap_or_default();
        let header_url = actor.header_url.clone().unwrap_or_default();

        Self {
            id: remote_account_rest_id(&actor.actor_uri),
            username: actor.username.clone(),
            acct: format!("{}@{}", actor.username, actor.domain),
            display_name: actor.display_name.clone(),
            locked: false,
            bot: false,
            created_at: String::new(),
            note: actor.summary_html.clone(),
            url: profile_url,
            avatar: avatar_url.clone(),
            avatar_static: avatar_url,
            header: header_url.clone(),
            header_static: header_url,
            fields: Vec::new(),
            followers_count: 0,
            following_count: 0,
            statuses_count: 0,
            source: None,
        }
    }
}
