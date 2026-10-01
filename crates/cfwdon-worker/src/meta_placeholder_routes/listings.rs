use crate::accounts::{load_account_stats, parse_relationship_query_ids};
use crate::activitypub::is_public_activitypub_visibility;
use crate::auth::{find_account_by_id, find_authenticated_local_account};
use crate::db_session::bind_request_d1;
use crate::media::find_media_attachments_by_status_id;
use crate::observability::log_json_event;
use crate::remote::{AccountReference, resolve_account_reference_with_fetch};
use crate::response::{
    RemoteCollectionFetchContext, enrich_remote_account_response,
    fetch_remote_actor_profile_with_context, reconcile_remote_account_status_summary,
};
use crate::responses::MastodonAccountResponse;
use crate::runtime_config::load_config;
use crate::statuses::{
    ResolvedStatus, build_local_status_response, build_remote_status_response,
    can_view_local_status, load_in_reply_to_account_id, resolve_status_reference,
};
use crate::store::remote::{find_remote_actor_by_actor_uri, upsert_remote_actor};
use worker::{Request, Response, Result, RouteContext};

pub(crate) async fn statuses_index_placeholder_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    let mut response = Vec::new();

    for status_id in parse_relationship_query_ids(&req)? {
        let Some(status) = resolve_status_reference(&db, &config, &status_id).await? else {
            continue;
        };

        match status {
            ResolvedStatus::Local(status) => {
                let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
                    continue;
                };
                if !can_view_local_status(&db, &status, viewer.as_ref(), &account).await? {
                    continue;
                }

                let media = find_media_attachments_by_status_id(&db, &status.id).await?;
                let in_reply_to_account_id = load_in_reply_to_account_id(&db, &status).await?;
                response.push(
                    build_local_status_response(
                        &db,
                        &config,
                        viewer.as_ref(),
                        &status,
                        &account,
                        in_reply_to_account_id,
                        media,
                    )
                    .await?,
                );
            }
            ResolvedStatus::Remote(status) => {
                if !is_public_activitypub_visibility(status.visibility.as_str()) {
                    continue;
                }
                let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await?
                else {
                    continue;
                };
                response.push(
                    build_remote_status_response(&db, &config, viewer.as_ref(), &status, &actor)
                        .await?,
                );
            }
        }
    }

    Response::from_json(&response)
}

pub(crate) async fn accounts_index_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    let mut response = Vec::new();

    for account_id in parse_relationship_query_ids(&req)? {
        let fetch_context = RemoteCollectionFetchContext::public(&config, &db, viewer.as_ref());
        match resolve_account_reference_with_fetch(&db, &account_id, Some(&fetch_context)).await? {
            Some(AccountReference::Local(account)) => {
                let stats = load_account_stats(&db, account.id()).await?;
                response.push(MastodonAccountResponse::from_account_with_stats(
                    &account, &config, &stats,
                ));
            }
            Some(AccountReference::Remote(actor)) => {
                let fetched =
                    fetch_remote_actor_profile_with_context(&actor.actor_uri, Some(&fetch_context))
                        .await;
                let mut account = match fetched.as_ref() {
                    Ok(fetched) => {
                        if let Err(error) = upsert_remote_actor(&db, &fetched.profile).await {
                            log_json_event(serde_json::json!({
                                "event": "remote_actor_upsert_failed",
                                "actor_uri": fetched.profile.actor_uri,
                                "error": error.to_string(),
                            }));
                        }
                        match find_remote_actor_by_actor_uri(&db, &fetched.profile.actor_uri)
                            .await?
                        {
                            Some(cached) => MastodonAccountResponse::from_remote_actor(&cached),
                            None => {
                                MastodonAccountResponse::from_remote_actor_profile(&fetched.profile)
                            }
                        }
                    }
                    Err(error) => {
                        log_json_event(serde_json::json!({
                            "event": "remote_actor_refresh_failed",
                            "actor_uri": actor.actor_uri,
                            "error": error.to_string(),
                        }));
                        MastodonAccountResponse::from_remote_actor(&actor)
                    }
                };
                if let Ok(fetched) = fetched.as_ref() {
                    let social_counts_updated_at =
                        find_remote_actor_by_actor_uri(&db, &fetched.profile.actor_uri)
                            .await?
                            .and_then(|row| row.social_counts_updated_at);
                    enrich_remote_account_response(
                        &db,
                        &fetched.profile.actor_uri,
                        social_counts_updated_at.as_deref(),
                        &mut account,
                        &fetched.document,
                        Some(&fetch_context),
                    )
                    .await?;
                } else if let Err(error) =
                    reconcile_remote_account_status_summary(&db, &actor.actor_uri, &mut account)
                        .await
                {
                    log_json_event(serde_json::json!({
                        "event": "remote_account_enrichment_failed",
                        "actor_uri": actor.actor_uri,
                        "stage": "status_summary",
                        "error": error.to_string(),
                    }));
                }
                response.push(account);
            }
            None => {}
        }
    }

    Response::from_json(&response)
}
