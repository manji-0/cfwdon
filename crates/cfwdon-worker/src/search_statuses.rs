use crate::find_media_attachments_by_status_id;
use crate::parse_remote_http_url;
use crate::{
    AccountReference, MastodonStatusResponse, build_local_status_response,
    build_remote_status_response, can_view_local_status, find_local_status_by_object_uri,
    is_public_activitypub_visibility, load_in_reply_to_account_id, resolve_account_reference,
    resolve_remote_status_by_url, search_local_status_rows, search_remote_status_rows,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::{D1Database, Result};

pub(crate) async fn resolve_search_status(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &str,
) -> Result<Option<MastodonStatusResponse>> {
    let query = query.trim();
    if parse_remote_http_url(query).is_err() {
        return Ok(None);
    }

    if let Some(status) = find_local_status_by_object_uri(db, config, query).await? {
        let Some(account) = crate::find_account_by_id(db, &status.account_id).await? else {
            return Ok(None);
        };
        if !can_view_local_status(db, &status, Some(viewer), &account).await? {
            return Ok(None);
        }
        let media = find_media_attachments_by_status_id(db, &status.id).await?;
        return Ok(Some(
            build_local_status_response(
                db,
                config,
                Some(viewer),
                &status,
                &account,
                load_in_reply_to_account_id(db, &status).await?,
                media,
            )
            .await?,
        ));
    }

    if let Some((status, actor)) = resolve_remote_status_by_url(db, config, query).await? {
        if !is_public_activitypub_visibility(&status.visibility) {
            return Ok(None);
        }
        return Ok(Some(
            build_remote_status_response(db, config, Some(viewer), &status, &actor).await?,
        ));
    }

    Ok(None)
}

pub(crate) async fn search_statuses_for_v2(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &str,
    limit: u32,
    offset: u32,
    account_id: Option<&str>,
) -> Result<Vec<MastodonStatusResponse>> {
    let account_reference = match account_id {
        Some(account_id) => resolve_account_reference(db, account_id).await?,
        None => None,
    };
    if account_id.is_some() && account_reference.is_none() {
        return Ok(Vec::new());
    }
    let query_limit = limit.saturating_add(offset).clamp(limit, 80);
    let mut entries = Vec::new();

    if !matches!(
        account_reference.as_ref(),
        Some(AccountReference::Remote(_))
    ) {
        let local_account_filter = match account_reference.as_ref() {
            Some(AccountReference::Local(account)) => Some(account.id.as_str()),
            _ => None,
        };
        for status in search_local_status_rows(db, query, query_limit, local_account_filter).await?
        {
            let Some(owner) = crate::find_account_by_id(db, &status.account_id).await? else {
                continue;
            };
            if !can_view_local_status(db, &status, Some(viewer), &owner).await? {
                continue;
            }
            let media = find_media_attachments_by_status_id(db, &status.id).await?;
            let in_reply_to_account_id = load_in_reply_to_account_id(db, &status).await?;
            entries.push((
                status.created_at.clone(),
                build_local_status_response(
                    db,
                    config,
                    Some(viewer),
                    &status,
                    &owner,
                    in_reply_to_account_id,
                    media,
                )
                .await?,
            ));
        }
    }

    if !matches!(account_reference.as_ref(), Some(AccountReference::Local(_))) {
        let remote_actor_filter = match account_reference.as_ref() {
            Some(AccountReference::Remote(actor)) => Some(actor.actor_uri.as_str()),
            _ => None,
        };
        for (status, actor) in
            search_remote_status_rows(db, query, query_limit, remote_actor_filter).await?
        {
            if !is_public_activitypub_visibility(&status.visibility) {
                continue;
            }
            entries.push((
                status.published_at.clone(),
                build_remote_status_response(db, config, Some(viewer), &status, &actor).await?,
            ));
        }
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0));
    Ok(entries
        .into_iter()
        .skip(offset as usize)
        .map(|(_, value)| value)
        .take(limit as usize)
        .collect())
}
