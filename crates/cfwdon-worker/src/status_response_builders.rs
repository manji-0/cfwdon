use crate::{
    AppConfig, LocalAccount, MastodonStatusResponse, MediaAttachmentRow, RemoteActorRow,
    RemoteStatusRow, StatusRow, actor_url, build_remote_status_card_value, build_status_card_value,
    can_view_local_status, count_local_status_favourites, count_local_status_reblogs,
    count_remote_status_favourites, count_remote_status_reblogs, count_rows,
    effective_remote_status_quote_state, effective_status_quote_state, find_account_by_id,
    find_local_status_by_object_uri, find_media_attachments_by_status_id, find_oauth_app_by_id,
    find_remote_actor_by_actor_uri, find_remote_status_attachments_by_status_id,
    find_remote_status_by_url_or_object_uri, has_remote_status_edit_snapshots, is_blocking_actor,
    is_local_follower_authorized, is_local_status_bookmarked_by, is_local_status_favourited_by,
    is_local_status_pinned_by, is_local_status_reblogged_by, is_local_status_thread_muted_by,
    is_muted_actor, is_remote_status_bookmarked_by, is_remote_status_favourited_by,
    is_remote_status_reblogged_by, load_in_reply_to_account_id, load_mastodon_poll_response,
    load_remote_mastodon_poll_response, load_remote_status_updated_at, load_status_filtered,
    load_status_updated_at, strip_html_tags,
};
use worker::{D1Database, Result, d1::D1Type};

async fn build_status_application(
    db: &D1Database,
    application_id: Option<i64>,
) -> Result<Option<serde_json::Value>> {
    let Some(application_id) = application_id else {
        return Ok(None);
    };
    let Some(app) = find_oauth_app_by_id(db, application_id).await? else {
        return Ok(None);
    };
    Ok(Some(serde_json::json!({
        "name": app.name,
        "website": app.website,
    })))
}

pub(crate) fn quote_document_with_state(
    state: &str,
    quoted_status: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "state": state,
        "quoted_status": quoted_status,
    })
}

pub(crate) fn pending_quote_document() -> serde_json::Value {
    quote_placeholder_document("pending")
}

pub(crate) fn quote_placeholder_document(state: &str) -> serde_json::Value {
    serde_json::json!({
        "state": state,
        "quoted_status": serde_json::Value::Null,
    })
}

fn quote_document_for_local_state(local_quote_state: Option<&str>) -> Option<serde_json::Value> {
    match local_quote_state {
        Some("pending") => Some(pending_quote_document()),
        Some(state @ ("revoked" | "rejected" | "unauthorized" | "deleted")) => {
            Some(quote_placeholder_document(state))
        }
        _ => None,
    }
}

fn quote_document_from_response(
    state: &str,
    response: MastodonStatusResponse,
) -> serde_json::Value {
    quote_document_with_state(
        state,
        serde_json::to_value(response).unwrap_or(serde_json::Value::Null),
    )
}

fn accepted_status_quotes_count_sql() -> &'static str {
    "SELECT COALESCE(SUM(count), 0) AS count
     FROM (
         SELECT COUNT(*) AS count
         FROM statuses
         WHERE quote_of_uri = ?1
           AND quote_state = 'accepted'
         UNION ALL
         SELECT COUNT(*) AS count
         FROM remote_statuses
         WHERE quote_of_uri = ?1
           AND quote_state = 'accepted'
     )"
}

async fn count_status_quotes_by_uri(db: &D1Database, status_uri: &str) -> Result<u64> {
    count_rows(db, accepted_status_quotes_count_sql(), status_uri).await
}

async fn viewer_blocks_domain(db: &D1Database, account_id: &str, domain: &str) -> Result<bool> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(domain)];
    let row = db
        .prepare(
            "SELECT 1 AS found
             FROM account_domain_blocks
             WHERE account_id = ?1
               AND domain = ?2
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row.is_some())
}

async fn quote_state_for_local_quoted_status(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    quoted_account: &LocalAccount,
) -> Result<Option<&'static str>> {
    let quoted_actor_uri = actor_url(config, &quoted_account.username);
    if is_blocking_actor(db, &viewer.id, &quoted_actor_uri).await? {
        return Ok(Some("blocked_account"));
    }
    if is_muted_actor(db, &viewer.id, &quoted_actor_uri).await? {
        return Ok(Some("muted_account"));
    }
    Ok(None)
}

async fn quote_state_for_remote_quoted_status(
    db: &D1Database,
    viewer: &LocalAccount,
    actor: &RemoteActorRow,
) -> Result<Option<&'static str>> {
    if is_blocking_actor(db, &viewer.id, &actor.actor_uri).await? {
        return Ok(Some("blocked_account"));
    }
    if viewer_blocks_domain(db, &viewer.id, &actor.domain).await? {
        return Ok(Some("blocked_domain"));
    }
    if is_muted_actor(db, &viewer.id, &actor.actor_uri).await? {
        return Ok(Some("muted_account"));
    }
    Ok(None)
}

async fn local_quoted_status_document_state(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    local_account: &LocalAccount,
) -> Result<&'static str> {
    let Some(viewer) = viewer else {
        return Ok("accepted");
    };
    Ok(
        quote_state_for_local_quoted_status(db, config, viewer, local_account)
            .await?
            .unwrap_or("accepted"),
    )
}

async fn remote_quoted_status_document_state(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    actor: &RemoteActorRow,
) -> Result<&'static str> {
    let Some(viewer) = viewer else {
        return Ok("accepted");
    };
    Ok(quote_state_for_remote_quoted_status(db, viewer, actor)
        .await?
        .unwrap_or("accepted"))
}

pub(crate) fn effective_local_quote_approval_policy(status: &StatusRow) -> &str {
    if matches!(status.visibility.as_str(), "private" | "direct") {
        "nobody"
    } else {
        status.quote_approval_policy.as_deref().unwrap_or("public")
    }
}

async fn build_local_quote_approval(
    db: &D1Database,
    status: &StatusRow,
    viewer: Option<&LocalAccount>,
    owner: &LocalAccount,
) -> Result<serde_json::Value> {
    let policy = effective_local_quote_approval_policy(status);
    let automatic = match policy {
        "public" => vec![serde_json::json!("public")],
        "followers" => vec![serde_json::json!("followers")],
        _ => Vec::new(),
    };
    let current_user = match policy {
        "public" => "automatic",
        "followers" => {
            if viewer.map(|viewer| viewer.id == owner.id).unwrap_or(false) {
                "automatic"
            } else if let Some(viewer) = viewer {
                if is_local_follower_authorized(db, &viewer.id, &owner.id).await? {
                    "automatic"
                } else {
                    "denied"
                }
            } else {
                "denied"
            }
        }
        _ => {
            if viewer.map(|viewer| viewer.id == owner.id).unwrap_or(false) {
                "automatic"
            } else {
                "denied"
            }
        }
    };

    Ok(serde_json::json!({
        "automatic": automatic,
        "manual": [],
        "current_user": current_user,
    }))
}

fn build_remote_quote_approval(status: &RemoteStatusRow) -> serde_json::Value {
    if !matches!(status.visibility.as_str(), "public" | "unlisted") {
        return serde_json::json!({
            "automatic": [],
            "manual": [],
            "current_user": "denied",
        });
    }

    serde_json::json!({
        "automatic": [],
        "manual": ["unsupported_policy"],
        "current_user": "manual",
    })
}

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
    build_local_status_response_inner(
        db,
        config,
        viewer,
        status,
        account,
        in_reply_to_account_id,
        media_attachments,
        true,
    )
    .await
}

async fn build_local_status_response_inner(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    if let Some(boost_of_uri) = status.boost_of_uri.as_deref() {
        return build_local_reblog_wrapper_response(
            db,
            config,
            viewer,
            status,
            account,
            in_reply_to_account_id,
            boost_of_uri,
            include_quote,
        )
        .await;
    }

    let mut response = MastodonStatusResponse::from_row(
        status,
        account,
        config,
        in_reply_to_account_id,
        media_attachments,
    );
    response.application = build_status_application(db, status.application_id).await?;
    response.card = build_status_card_value(&status._text_content);
    response.poll = load_mastodon_poll_response(db, &status.id, viewer).await?;
    response.mentions = build_status_mentions(db, config, &status._text_content).await?;
    response.favourites_count = count_local_status_favourites(db, &status.id).await?;
    response.favourited = match viewer {
        Some(viewer) => is_local_status_favourited_by(db, &viewer.id, status).await?,
        None => false,
    };
    response.reblogs_count = count_local_status_reblogs(db, &status.id).await?;
    response.quotes_count = count_status_quotes_by_uri(db, &response.uri).await?;
    response.reblogged = match viewer {
        Some(viewer) => is_local_status_reblogged_by(db, &viewer.id, status).await?,
        None => false,
    };
    response.bookmarked = match viewer {
        Some(viewer) => is_local_status_bookmarked_by(db, &viewer.id, status).await?,
        None => false,
    };
    response.pinned = match viewer {
        Some(viewer) => is_local_status_pinned_by(db, &viewer.id, &status.id).await?,
        None => false,
    };
    response.muted = match viewer {
        Some(viewer) => is_local_status_thread_muted_by(db, &viewer.id, status).await?,
        None => false,
    };
    response.edited_at = match load_status_updated_at(db, &status.id).await? {
        Some(updated_at) if updated_at != status.created_at => Some(updated_at),
        _ => None,
    };
    response.filtered = match viewer {
        Some(viewer) => {
            load_status_filtered(
                db,
                &viewer.id,
                &status.id,
                &status._text_content,
                &status.spoiler_text,
            )
            .await?
        }
        None => Vec::new(),
    };
    response.quote_approval = Some(build_local_quote_approval(db, status, viewer, account).await?);
    if include_quote {
        response.quote = build_quoted_status_value(
            db,
            config,
            viewer,
            status.quote_of_uri.as_deref(),
            Some(effective_status_quote_state(status)),
            true,
        )
        .await?;
    }
    Ok(response)
}

pub(crate) async fn build_remote_status_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
) -> Result<MastodonStatusResponse> {
    build_remote_status_response_inner(db, config, viewer, status, actor, true).await
}

async fn build_remote_status_response_inner(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    if let Some(boost_of_uri) = status.boost_of_uri.as_deref() {
        return build_remote_reblog_wrapper_response(
            db,
            config,
            viewer,
            status,
            actor,
            boost_of_uri,
            include_quote,
        )
        .await;
    }

    let mut response = MastodonStatusResponse::from_remote_row(status, actor, config);
    let text_content = strip_html_tags(&status.content_html);
    let remote_attachments = find_remote_status_attachments_by_status_id(db, &status.id).await?;
    response.card = build_remote_status_card_value(&text_content, &remote_attachments);
    response.media_attachments = remote_attachments
        .iter()
        .map(|media| {
            serde_json::to_value(crate::MastodonMediaAttachmentResponse::from_remote_row(
                media,
            ))
            .unwrap_or(serde_json::Value::Null)
        })
        .collect();
    response.mentions = build_status_mentions(db, config, &text_content).await?;
    response.favourites_count = count_remote_status_favourites(db, &status.id).await?;
    response.favourited = match viewer {
        Some(viewer) => is_remote_status_favourited_by(db, &viewer.id, &status.id).await?,
        None => false,
    };
    response.reblogs_count = count_remote_status_reblogs(db, &status.id).await?;
    response.quotes_count = count_status_quotes_by_uri(db, &response.uri).await?;
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
    if has_remote_status_edit_snapshots(db, &status.id).await? {
        response.edited_at = load_remote_status_updated_at(db, &status.id).await?;
    }
    response.filtered = match viewer {
        Some(viewer) => {
            load_status_filtered(
                db,
                &viewer.id,
                &status.id,
                &text_content,
                &status.spoiler_text,
            )
            .await?
        }
        None => Vec::new(),
    };
    response.quote_approval = Some(build_remote_quote_approval(status));
    if include_quote {
        response.quote = build_quoted_status_value(
            db,
            config,
            viewer,
            status.quote_of_uri.as_deref(),
            Some(effective_remote_status_quote_state(status)),
            false,
        )
        .await?;
    }
    Ok(response)
}

async fn build_quoted_status_value(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    quote_of_uri: Option<&str>,
    local_quote_state: Option<&str>,
    pending_remote_quote: bool,
) -> Result<Option<serde_json::Value>> {
    let Some(quote_of_uri) = quote_of_uri else {
        return Ok(None);
    };
    if let Some(document) = quote_document_for_local_state(local_quote_state) {
        return Ok(Some(document));
    }

    if let Some(local_status) = find_local_status_by_object_uri(db, config, quote_of_uri).await? {
        let Some(local_account) = find_account_by_id(db, &local_status.account_id).await? else {
            return Ok(None);
        };
        if !can_view_local_status(db, &local_status, viewer, &local_account).await? {
            return Ok(Some(quote_placeholder_document("unauthorized")));
        }
        let media = find_media_attachments_by_status_id(db, &local_status.id).await?;
        let mut response = MastodonStatusResponse::from_row(
            &local_status,
            &local_account,
            config,
            load_in_reply_to_account_id(db, &local_status).await?,
            media,
        );
        response.card = build_status_card_value(&local_status._text_content);
        response.poll = load_mastodon_poll_response(db, &local_status.id, viewer).await?;
        response.filtered = match viewer {
            Some(viewer) => {
                load_status_filtered(
                    db,
                    &viewer.id,
                    &local_status.id,
                    &local_status._text_content,
                    &local_status.spoiler_text,
                )
                .await?
            }
            None => Vec::new(),
        };
        response.mentions = build_status_mentions(db, config, &local_status._text_content).await?;
        response.favourites_count = count_local_status_favourites(db, &local_status.id).await?;
        response.favourited = match viewer {
            Some(viewer) => is_local_status_favourited_by(db, &viewer.id, &local_status).await?,
            None => false,
        };
        response.reblogs_count = count_local_status_reblogs(db, &local_status.id).await?;
        response.reblogged = match viewer {
            Some(viewer) => is_local_status_reblogged_by(db, &viewer.id, &local_status).await?,
            None => false,
        };
        response.bookmarked = match viewer {
            Some(viewer) => is_local_status_bookmarked_by(db, &viewer.id, &local_status).await?,
            None => false,
        };
        response.pinned = match viewer {
            Some(viewer) => is_local_status_pinned_by(db, &viewer.id, &local_status.id).await?,
            None => false,
        };
        response.muted = match viewer {
            Some(viewer) => is_local_status_thread_muted_by(db, &viewer.id, &local_status).await?,
            None => false,
        };
        response.quote = None;
        let state = local_quoted_status_document_state(db, config, viewer, &local_account).await?;
        return Ok(Some(quote_document_from_response(state, response)));
    }

    if let Some(remote_status) = find_remote_status_by_url_or_object_uri(db, quote_of_uri).await? {
        if pending_remote_quote {
            return Ok(Some(pending_quote_document()));
        }
        if !matches!(remote_status.visibility.as_str(), "public" | "unlisted") {
            return Ok(Some(quote_placeholder_document("unauthorized")));
        }
        let Some(actor) = find_remote_actor_by_actor_uri(db, &remote_status.actor_uri).await?
        else {
            return Ok(None);
        };
        let mut response = MastodonStatusResponse::from_remote_row(&remote_status, &actor, config);
        let text_content = strip_html_tags(&remote_status.content_html);
        let remote_attachments =
            find_remote_status_attachments_by_status_id(db, &remote_status.id).await?;
        response.card = build_remote_status_card_value(&text_content, &remote_attachments);
        response.media_attachments = remote_attachments
            .iter()
            .map(|media| {
                serde_json::to_value(crate::MastodonMediaAttachmentResponse::from_remote_row(
                    media,
                ))
                .unwrap_or(serde_json::Value::Null)
            })
            .collect();
        response.filtered = match viewer {
            Some(viewer) => {
                load_status_filtered(
                    db,
                    &viewer.id,
                    &remote_status.id,
                    &text_content,
                    &remote_status.spoiler_text,
                )
                .await?
            }
            None => Vec::new(),
        };
        response.mentions = build_status_mentions(db, config, &text_content).await?;
        response.favourites_count = count_remote_status_favourites(db, &remote_status.id).await?;
        response.favourited = match viewer {
            Some(viewer) => {
                is_remote_status_favourited_by(db, &viewer.id, &remote_status.id).await?
            }
            None => false,
        };
        response.reblogs_count = count_remote_status_reblogs(db, &remote_status.id).await?;
        response.reblogged = match viewer {
            Some(viewer) => {
                is_remote_status_reblogged_by(db, &viewer.id, &remote_status.id).await?
            }
            None => false,
        };
        response.bookmarked = match viewer {
            Some(viewer) => {
                is_remote_status_bookmarked_by(db, &viewer.id, &remote_status.id).await?
            }
            None => false,
        };
        response.muted = match viewer {
            Some(viewer) => is_muted_actor(db, &viewer.id, &actor.actor_uri).await?,
            None => false,
        };
        response.poll = load_remote_mastodon_poll_response(db, &remote_status, viewer).await?;
        response.quote = None;
        let state = remote_quoted_status_document_state(db, viewer, &actor).await?;
        return Ok(Some(quote_document_from_response(state, response)));
    }

    Ok(None)
}

async fn build_remote_reblog_wrapper_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    wrapper_status: &RemoteStatusRow,
    wrapper_actor: &RemoteActorRow,
    boost_of_uri: &str,
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    let embedded = if let Some(local_status) =
        find_local_status_by_object_uri(db, config, boost_of_uri).await?
    {
        if let Some(local_account) = find_account_by_id(db, &local_status.account_id).await? {
            if can_view_local_status(db, &local_status, viewer, &local_account).await? {
                let media = find_media_attachments_by_status_id(db, &local_status.id).await?;
                Some(
                    Box::pin(build_local_status_response_inner(
                        db,
                        config,
                        viewer,
                        &local_status,
                        &local_account,
                        load_in_reply_to_account_id(db, &local_status).await?,
                        media,
                        include_quote,
                    ))
                    .await?,
                )
            } else {
                None
            }
        } else {
            None
        }
    } else if let Some(remote_status) =
        find_remote_status_by_url_or_object_uri(db, boost_of_uri).await?
    {
        if !matches!(remote_status.visibility.as_str(), "public" | "unlisted") {
            None
        } else if let Some(actor) =
            find_remote_actor_by_actor_uri(db, &remote_status.actor_uri).await?
        {
            Some(
                Box::pin(build_remote_status_response_inner(
                    db,
                    config,
                    viewer,
                    &remote_status,
                    &actor,
                    include_quote,
                ))
                .await?,
            )
        } else {
            None
        }
    } else {
        None
    };

    let mut response = embedded.clone().unwrap_or_else(|| {
        MastodonStatusResponse::from_remote_row(wrapper_status, wrapper_actor, config)
    });
    response.id = wrapper_status.id.clone();
    response.created_at = wrapper_status.published_at.clone();
    response.in_reply_to_id = wrapper_status.in_reply_to_uri.clone();
    response.in_reply_to_account_id = None;
    response.visibility = wrapper_status.visibility.clone();
    response.uri = wrapper_status.object_uri.clone();
    response.url = wrapper_status
        .url
        .clone()
        .unwrap_or_else(|| wrapper_status.object_uri.clone());
    response.account = crate::MastodonAccountResponse::from_remote_actor(wrapper_actor);
    response.reblog =
        embedded.map(|status| serde_json::to_value(status).unwrap_or(serde_json::Value::Null));
    response.content.clear();
    response.text = None;
    response.media_attachments.clear();
    response.mentions.clear();
    response.tags.clear();
    response.emojis.clear();
    response.card = None;
    response.poll = None;
    response.quote = None;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_status_quotes_count_sql_counts_local_and_remote_quotes_once() {
        let sql = accepted_status_quotes_count_sql();

        assert_eq!(sql.matches("quote_of_uri = ?1").count(), 2);
        assert!(sql.contains("FROM statuses"));
        assert!(sql.contains("FROM remote_statuses"));
        assert!(sql.contains("UNION ALL"));
    }
}

async fn build_local_reblog_wrapper_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    wrapper_status: &StatusRow,
    wrapper_account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    boost_of_uri: &str,
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    let embedded = if let Some(local_status) =
        find_local_status_by_object_uri(db, config, boost_of_uri).await?
    {
        if let Some(local_account) = find_account_by_id(db, &local_status.account_id).await? {
            if can_view_local_status(db, &local_status, viewer, &local_account).await? {
                let media = find_media_attachments_by_status_id(db, &local_status.id).await?;
                Some(
                    Box::pin(build_local_status_response_inner(
                        db,
                        config,
                        viewer,
                        &local_status,
                        &local_account,
                        load_in_reply_to_account_id(db, &local_status).await?,
                        media,
                        include_quote,
                    ))
                    .await?,
                )
            } else {
                None
            }
        } else {
            None
        }
    } else if let Some(remote_status) =
        find_remote_status_by_url_or_object_uri(db, boost_of_uri).await?
    {
        if !matches!(remote_status.visibility.as_str(), "public" | "unlisted") {
            None
        } else if let Some(actor) =
            find_remote_actor_by_actor_uri(db, &remote_status.actor_uri).await?
        {
            Some(
                build_remote_status_response_inner(
                    db,
                    config,
                    viewer,
                    &remote_status,
                    &actor,
                    include_quote,
                )
                .await?,
            )
        } else {
            None
        }
    } else {
        None
    };

    let mut response = embedded.clone().unwrap_or_else(|| {
        MastodonStatusResponse::from_row(
            wrapper_status,
            wrapper_account,
            config,
            in_reply_to_account_id.clone(),
            Vec::new(),
        )
    });
    response.id = wrapper_status.id.clone();
    response.created_at = wrapper_status.created_at.clone();
    response.in_reply_to_id = wrapper_status.in_reply_to_id.clone();
    response.in_reply_to_account_id = in_reply_to_account_id;
    response.visibility = wrapper_status.visibility.clone();
    response.uri = wrapper_status.ap_id.clone().unwrap_or_else(|| {
        format!(
            "{}/statuses/{}",
            actor_url(config, &wrapper_account.username),
            wrapper_status.id
        )
    });
    response.url = response.uri.clone();
    response.account = crate::MastodonAccountResponse::from_account(wrapper_account, config);
    response.reblog = embedded
        .clone()
        .map(|status| serde_json::to_value(status).unwrap_or(serde_json::Value::Null));
    response.content.clear();
    response.text = None;
    response.media_attachments.clear();
    response.mentions.clear();
    response.tags.clear();
    response.emojis.clear();
    response.card = None;
    response.poll = None;
    response.quote = None;
    Ok(response)
}
