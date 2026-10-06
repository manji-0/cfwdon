use super::filters::{account_status_list_options, local_status_matches_account_filters};
use super::html::{account_statuses_html_response, local_status_html_item};
use super::pagination::account_statuses_older_page_url;
use crate::identity::actor_url;
use crate::media::find_media_attachments_by_status_ids;
use crate::relationship::is_local_follower_authorized;
use crate::statuses::{
    AccountStatusVisibilityScope, AccountStatusesQuery, can_view_local_status,
    list_account_statuses, list_pinned_statuses_for_account, list_public_account_statuses,
    load_in_reply_to_account_ids,
};
use crate::timelines::{StatusRenderItem, build_timeline_link_header, render_status_items};
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::{LocalAccount, LocalStatus};
use std::collections::HashMap;
use worker::{Request, Response, Result};

struct LocalAccountStatusPage {
    statuses: Vec<LocalStatus>,
    older_page_url: Option<String>,
}

async fn load_local_account_status_page(
    req: &Request,
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    account: &LocalAccount,
    query: &AccountStatusesQuery,
    limit: u32,
    query_limit: u32,
    wants_html: bool,
    min_id: Option<&str>,
) -> Result<LocalAccountStatusPage> {
    let is_pinned_page = query.pinned.unwrap_or(false);
    let html_fetch_limit = limit.saturating_add(1);
    let mut statuses = if is_pinned_page {
        list_pinned_statuses_for_account(db, account.id()).await?
    } else if wants_html {
        list_public_account_statuses(
            db,
            account.id(),
            query.max_id.as_deref(),
            min_id,
            html_fetch_limit,
        )
        .await?
    } else {
        let visibility = match viewer {
            Some(viewer) if viewer.id() == account.id() => AccountStatusVisibilityScope::All,
            Some(viewer) if is_local_follower_authorized(db, viewer.id(), account.id()).await? => {
                AccountStatusVisibilityScope::PublicUnlistedPrivate
            }
            _ => AccountStatusVisibilityScope::Public,
        };
        list_account_statuses(
            db,
            account.id(),
            account_status_list_options(query, min_id, query_limit, visibility),
        )
        .await?
    };
    let older_page_url = if wants_html && !is_pinned_page && statuses.len() > limit as usize {
        statuses.truncate(limit as usize);
        statuses
            .last()
            .map(|status| account_statuses_older_page_url(req, limit, &status.id))
            .transpose()?
    } else {
        None
    };

    Ok(LocalAccountStatusPage {
        statuses,
        older_page_url,
    })
}

async fn respond_local_account_statuses_html(
    config: &AppConfig,
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    account: &LocalAccount,
    query: &AccountStatusesQuery,
    limit: u32,
    page: LocalAccountStatusPage,
) -> Result<Response> {
    let status_ids = page
        .statuses
        .iter()
        .map(|status| status.id.clone())
        .collect::<Vec<_>>();
    let mut media_by_status_id = find_media_attachments_by_status_ids(db, &status_ids).await?;
    let exclude_replies = query.exclude_replies.unwrap_or(false);
    let in_reply_to_account_ids = if exclude_replies {
        load_in_reply_to_account_ids(db, &page.statuses).await?
    } else {
        HashMap::new()
    };
    let mut html_statuses = Vec::new();

    for status in page.statuses.into_iter().take(limit as usize) {
        if !can_view_local_status(db, &status, viewer, account).await? {
            continue;
        }
        let media = media_by_status_id.remove(&status.id).unwrap_or_default();
        if !local_status_matches_account_filters(
            &status,
            account.id(),
            query,
            &media,
            status
                .in_reply_to_id
                .as_ref()
                .and_then(|_| in_reply_to_account_ids.get(&status.id)),
        ) {
            continue;
        }

        html_statuses.push(local_status_html_item(config, account, &status, &media));
    }

    account_statuses_html_response(
        config,
        account.display_name(),
        account.username(),
        &actor_url(config, account.username()),
        &html_statuses,
        page.older_page_url.as_deref(),
    )
}

/// Render the JSON page: visibility and account filters first, then one
/// batched render of the survivors through the timeline preloads.
#[allow(clippy::too_many_arguments)]
async fn respond_local_account_statuses_json(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    account: &LocalAccount,
    query: &AccountStatusesQuery,
    limit: u32,
    statuses: Vec<LocalStatus>,
) -> Result<Response> {
    let statuses = statuses
        .into_iter()
        .take(limit as usize)
        .collect::<Vec<_>>();
    // Page links follow the fetched window, so a page whose statuses are all
    // filtered out still points past itself.
    let link = build_timeline_link_header(
        req,
        limit,
        statuses.first().map(|status| status.id.as_str()),
        statuses.last().map(|status| status.id.as_str()),
    )?;
    let status_ids = statuses
        .iter()
        .map(|status| status.id.clone())
        .collect::<Vec<_>>();
    let (media_by_status_id, in_reply_to_account_ids, visible) = futures_util::try_join!(
        find_media_attachments_by_status_ids(db, &status_ids),
        load_in_reply_to_account_ids(db, &statuses),
        futures_util::future::try_join_all(
            statuses
                .iter()
                .map(|status| can_view_local_status(db, status, viewer, account)),
        ),
    )?;

    let items = statuses
        .into_iter()
        .zip(visible)
        .filter(|(status, visible)| {
            *visible
                && local_status_matches_account_filters(
                    status,
                    account.id(),
                    query,
                    media_by_status_id
                        .get(&status.id)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    status
                        .in_reply_to_id
                        .as_ref()
                        .and_then(|_| in_reply_to_account_ids.get(&status.id)),
                )
        })
        .map(|(status, _)| StatusRenderItem::Local(status))
        .collect::<Vec<_>>();

    let mut builder = Response::builder();
    if let Some(link) = link {
        builder = builder.with_header("Link", &link)?;
    }
    builder.from_json(&render_status_items(db, config, viewer, items).await?)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn local_account_statuses_response(
    req: &Request,
    config: &AppConfig,
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    account: LocalAccount,
    query: &AccountStatusesQuery,
    limit: u32,
    query_limit: u32,
    wants_html: bool,
    min_id: Option<&str>,
) -> Result<Response> {
    let page = load_local_account_status_page(
        req,
        db,
        viewer,
        &account,
        query,
        limit,
        query_limit,
        wants_html,
        min_id,
    )
    .await?;

    if wants_html {
        return respond_local_account_statuses_html(
            config, db, viewer, &account, query, limit, page,
        )
        .await;
    }

    respond_local_account_statuses_json(
        req,
        db,
        config,
        viewer,
        &account,
        query,
        limit,
        page.statuses,
    )
    .await
}
