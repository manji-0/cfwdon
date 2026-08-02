use super::filters::{account_status_list_options, local_status_matches_account_filters};
use super::html::{account_statuses_html_response, local_status_html_item};
use super::pagination::account_statuses_older_page_url;
use crate::{
    AccountStatusVisibilityScope, AccountStatusesQuery, AppConfig, LocalAccount, Request, Response,
    Result, actor_url, build_local_status_response_with_quote_count_preloads,
    can_view_local_status, find_media_attachments_by_status_ids, is_local_follower_authorized,
    list_account_statuses, list_pinned_statuses_for_account, list_public_account_statuses,
    load_account_filter_matcher, load_in_reply_to_account_ids, local_status_ap_id,
    preload_local_status_viewer_state, preload_mastodon_poll_responses,
    preload_status_applications, preload_status_counts, preload_status_quote_counts,
};

use crate::D1Database;
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
    let status_ids = statuses
        .iter()
        .map(|status| status.id.clone())
        .collect::<Vec<_>>();
    if wants_html {
        let mut media_by_status_id = find_media_attachments_by_status_ids(db, &status_ids).await?;
        let exclude_replies = query.exclude_replies.unwrap_or(false);
        let in_reply_to_account_ids = if exclude_replies {
            load_in_reply_to_account_ids(db, &statuses).await?
        } else {
            Default::default()
        };
        let mut html_statuses = Vec::new();

        for status in statuses.into_iter().take(limit as usize) {
            if !can_view_local_status(db, &status, viewer, &account).await? {
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

            html_statuses.push(local_status_html_item(config, &account, &status, &media));
        }

        return account_statuses_html_response(
            config,
            account.display_name(),
            account.username(),
            &actor_url(config, account.username()),
            &html_statuses,
            older_page_url.as_deref(),
        );
    }

    let status_refs = statuses.iter().collect::<Vec<_>>();
    let quote_uris = statuses
        .iter()
        .map(|status| local_status_ap_id(config, &account, status))
        .collect::<Vec<_>>();
    let (
        counts_preload,
        quote_counts_preload,
        poll_preload,
        viewer_state_preload,
        application_preload,
        mut media_by_status_id,
        in_reply_to_account_ids,
        filter_matcher,
    ) = futures_util::try_join!(
        preload_status_counts(db, &status_ids, &[]),
        preload_status_quote_counts(db, &quote_uris),
        preload_mastodon_poll_responses(db, &status_ids, viewer),
        async {
            match viewer {
                Some(viewer) => {
                    preload_local_status_viewer_state(db, viewer.id(), &status_refs, None).await
                }
                None => Ok(Default::default()),
            }
        },
        preload_status_applications(db, config, &status_refs),
        find_media_attachments_by_status_ids(db, &status_ids),
        load_in_reply_to_account_ids(db, &statuses),
        async {
            match viewer {
                Some(viewer) => load_account_filter_matcher(db, viewer.id()).await.map(Some),
                None => Ok(None),
            }
        },
    )?;
    let mut response = Vec::new();

    for status in statuses.into_iter().take(limit as usize) {
        if !can_view_local_status(db, &status, viewer, &account).await? {
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

        response.push(
            build_local_status_response_with_quote_count_preloads(
                db,
                config,
                viewer,
                &status,
                &account,
                in_reply_to_account_ids.get(&status.id).cloned(),
                media,
                filter_matcher.as_ref(),
                Some(&counts_preload),
                Some(&quote_counts_preload),
                Some(&poll_preload),
                Some(&viewer_state_preload),
                Some(&application_preload),
            )
            .await?,
        );
    }

    Response::from_json(&response)
}
