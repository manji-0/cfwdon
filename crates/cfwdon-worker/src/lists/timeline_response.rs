use super::{
    ListTimelineQuery, list_id_from_context, list_membership_refs,
    list_membership_variants_for_local_account, list_membership_variants_for_remote_actor,
    list_row_by_id,
};
use crate::auth::find_account_by_id;
use crate::media::find_media_attachments_by_status_id;
use crate::profile::require_authenticated_local_account;
use crate::relationship::is_muted_actor;
use crate::runtime_config::load_config;
use crate::statuses::{
    build_local_status_response, build_remote_status_response, list_local_public_timeline_statuses,
    list_remote_public_timeline_statuses, load_in_reply_to_account_id,
    local_status_ids_thread_muted_by,
};
use crate::timelines::{
    build_timeline_link_header, resolve_timeline_cursor, timeline_fetch_limit, timeline_limit,
};
use std::collections::HashSet;
use worker::{Request, Response, Result, RouteContext};

pub(crate) async fn list_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: ListTimelineQuery = req.query().unwrap_or_default();
    let pagination = query.pagination();
    let limit = timeline_limit(&pagination);
    let query_limit = timeline_fetch_limit(limit);
    let list_id = list_id_from_context(&ctx)?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(list) = list_row_by_id(&db, account.id(), &list_id).await? else {
        return Response::error("list not found", 404);
    };
    let cursor = resolve_timeline_cursor(&db, &pagination).await?;
    let membership_refs = list_membership_refs(&db, &list_id)
        .await?
        .into_iter()
        .map(|row| row.target_account_ref)
        .collect::<HashSet<_>>();
    let mut entries = Vec::new();
    let local_statuses = list_local_public_timeline_statuses(&db, &cursor, query_limit).await?;
    let muted_local_status_ids = local_status_ids_thread_muted_by(
        &db,
        account.id(),
        &local_statuses.iter().collect::<Vec<_>>(),
    )
    .await?;

    for status in local_statuses {
        let Some(author) = find_account_by_id(&db, &status.account_id).await? else {
            continue;
        };
        if !list_membership_variants_for_local_account(&author, &config)
            .into_iter()
            .any(|candidate| membership_refs.contains(&candidate))
        {
            continue;
        }
        if list.replies_policy == "none" && status.in_reply_to_id.is_some() {
            continue;
        }
        if muted_local_status_ids.contains(&status.id) {
            continue;
        }
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        entries.push((
            status.created_at.clone(),
            status.id.clone(),
            build_local_status_response(
                &db,
                &config,
                Some(&account),
                &status,
                &author,
                load_in_reply_to_account_id(&db, &status).await?,
                media,
            )
            .await?,
        ));
    }

    for (status, actor) in list_remote_public_timeline_statuses(&db, &cursor, query_limit).await? {
        if !list_membership_variants_for_remote_actor(&actor)
            .into_iter()
            .any(|candidate| membership_refs.contains(&candidate))
        {
            continue;
        }
        if list.replies_policy == "none" && status.in_reply_to_uri.is_some() {
            continue;
        }
        if is_muted_actor(&db, account.id(), &actor.actor_uri).await? {
            continue;
        }
        entries.push((
            status.published_at.clone(),
            status.id.clone(),
            build_remote_status_response(&db, &config, Some(&account), &status, &actor).await?,
        ));
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    let first_id = entries
        .first()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let last_id = entries
        .last()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let response = entries
        .into_iter()
        .map(|(_, _, status)| status)
        .take(limit as usize)
        .collect::<Vec<_>>();
    let mut builder = Response::from_json(&response)?;
    if let Some(link) =
        build_timeline_link_header(&req, limit, first_id.as_deref(), last_id.as_deref())?
    {
        builder.headers_mut().set("Link", &link)?;
    }
    Ok(builder)
}
