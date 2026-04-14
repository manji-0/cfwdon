use super::{
    AccountReference, AccountStatusesQuery, Error, Request, Response, Result, RouteContext,
    build_local_status_response, build_remote_status_response, can_view_local_status,
    find_authenticated_local_account, find_media_attachments_by_status_id,
    is_public_activitypub_visibility, list_account_statuses, list_remote_statuses_by_actor_uri,
    load_config, load_in_reply_to_account_id, resolve_account_reference, status_contains_tag,
    status_is_reply_to_other_account,
};

pub(crate) async fn account_statuses_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;
    let query: AccountStatusesQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);

    let db = ctx.d1(&config.database_binding)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    match resolve_account_reference(&db, &account_id).await? {
        Some(AccountReference::Local(account)) => {
            let statuses = list_account_statuses(&db, &account.id, limit).await?;
            let mut response = Vec::new();

            for status in statuses {
                if !can_view_local_status(&db, &status, viewer.as_ref(), &account).await? {
                    continue;
                }
                if query.pinned.unwrap_or(false) {
                    continue;
                }
                if let Some(tag) = query.tagged.as_deref()
                    && !status_contains_tag(&status, tag)
                {
                    continue;
                }
                if query.exclude_reblogs.unwrap_or(false) {
                    // Local reblog support does not exist yet, so this filter is effectively a no-op.
                }
                if query.exclude_replies.unwrap_or(false)
                    && status_is_reply_to_other_account(&db, &status, &account.id).await?
                {
                    continue;
                }

                let media = find_media_attachments_by_status_id(&db, &status.id).await?;
                if query.only_media.unwrap_or(false) && media.is_empty() {
                    continue;
                }

                response.push(
                    build_local_status_response(
                        &db,
                        &config,
                        viewer.as_ref(),
                        &status,
                        &account,
                        load_in_reply_to_account_id(&db, &status).await?,
                        media,
                    )
                    .await?,
                );
            }

            Response::from_json(&response)
        }
        Some(AccountReference::Remote(actor)) => {
            let mut response = Vec::new();
            for status in list_remote_statuses_by_actor_uri(&db, &actor.actor_uri, limit).await? {
                if !is_public_activitypub_visibility(&status.visibility) {
                    continue;
                }
                if query.pinned.unwrap_or(false) {
                    continue;
                }
                if let Some(tag) = query.tagged.as_deref()
                    && !status
                        .content_html
                        .to_ascii_lowercase()
                        .contains(&tag.to_ascii_lowercase())
                {
                    continue;
                }
                if query.exclude_reblogs.unwrap_or(false) {
                    // Remote reblog parsing is not implemented yet.
                }
                if query.exclude_replies.unwrap_or(false) && status.in_reply_to_uri.is_some() {
                    continue;
                }
                if query.only_media.unwrap_or(false) {
                    continue;
                }

                response.push(
                    build_remote_status_response(&db, &config, viewer.as_ref(), &status, &actor)
                        .await?,
                );
            }
            Response::from_json(&response)
        }
        None => Response::error("account not found", 404),
    }
}
