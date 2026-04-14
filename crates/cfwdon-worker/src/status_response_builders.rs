use crate::{
    AppConfig, LocalAccount, MastodonStatusResponse, MediaAttachmentRow, RemoteActorRow,
    RemoteStatusRow, StatusRow, actor_url, count_local_status_favourites,
    count_local_status_reblogs, count_remote_status_favourites, count_remote_status_reblogs,
    is_local_status_bookmarked_by, is_local_status_favourited_by, is_local_status_reblogged_by,
    is_muted_actor, is_remote_status_bookmarked_by, is_remote_status_favourited_by,
    is_remote_status_reblogged_by, load_mastodon_poll_response, load_remote_mastodon_poll_response,
    strip_html_tags,
};
use worker::{D1Database, Result};

pub(crate) async fn build_status_mentions(
    db: &D1Database,
    config: &AppConfig,
    text: &str,
) -> Result<Vec<serde_json::Value>> {
    let mut mentions = Vec::new();

    for handle in crate::extract_account_handles_from_text(text, config) {
        if handle.is_local_to(&config.instance_domain) {
            let Some(account) = crate::find_account_by_username(db, &handle.username).await? else {
                continue;
            };
            mentions.push(serde_json::json!({
                "id": account.id,
                "username": account.username,
                "url": actor_url(config, &account.username),
                "acct": account.acct(),
            }));
            continue;
        }

        let Some(domain) = handle.domain.as_deref() else {
            continue;
        };
        let Some(actor) =
            crate::find_remote_actor_by_username_domain(db, &handle.username, domain).await?
        else {
            continue;
        };
        mentions.push(serde_json::json!({
            "id": crate::remote_account_rest_id(&actor.actor_uri),
            "username": actor.username,
            "url": actor.profile_url.clone().unwrap_or_else(|| actor.actor_uri.clone()),
            "acct": format!("{}@{}", actor.username, actor.domain),
        }));
    }

    Ok(mentions)
}

pub(crate) async fn build_local_status_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
) -> Result<MastodonStatusResponse> {
    let mut response = MastodonStatusResponse::from_row(
        status,
        account,
        config,
        in_reply_to_account_id,
        media_attachments,
    );
    response.poll = load_mastodon_poll_response(db, &status.id, viewer).await?;
    response.mentions = build_status_mentions(db, config, &status._text_content).await?;
    response.favourites_count = count_local_status_favourites(db, &status.id).await?;
    response.favourited = match viewer {
        Some(viewer) => is_local_status_favourited_by(db, &viewer.id, status).await?,
        None => false,
    };
    response.reblogs_count = count_local_status_reblogs(db, &status.id).await?;
    response.reblogged = match viewer {
        Some(viewer) => is_local_status_reblogged_by(db, &viewer.id, status).await?,
        None => false,
    };
    response.bookmarked = match viewer {
        Some(viewer) => is_local_status_bookmarked_by(db, &viewer.id, status).await?,
        None => false,
    };
    response.muted = match viewer {
        Some(viewer) => {
            is_muted_actor(db, &viewer.id, &actor_url(config, &account.username)).await?
        }
        None => false,
    };
    Ok(response)
}

pub(crate) async fn build_remote_status_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
) -> Result<MastodonStatusResponse> {
    let mut response = MastodonStatusResponse::from_remote_row(status, actor, config);
    let text_content = strip_html_tags(&status.content_html);
    response.mentions = build_status_mentions(db, config, &text_content).await?;
    response.favourites_count = count_remote_status_favourites(db, &status.id).await?;
    response.favourited = match viewer {
        Some(viewer) => is_remote_status_favourited_by(db, &viewer.id, &status.id).await?,
        None => false,
    };
    response.reblogs_count = count_remote_status_reblogs(db, &status.id).await?;
    response.reblogged = match viewer {
        Some(viewer) => is_remote_status_reblogged_by(db, &viewer.id, &status.id).await?,
        None => false,
    };
    response.bookmarked = match viewer {
        Some(viewer) => is_remote_status_bookmarked_by(db, &viewer.id, &status.id).await?,
        None => false,
    };
    response.muted = match viewer {
        Some(viewer) => is_muted_actor(db, &viewer.id, &actor.actor_uri).await?,
        None => false,
    };
    response.poll = load_remote_mastodon_poll_response(db, status, viewer).await?;
    Ok(response)
}
