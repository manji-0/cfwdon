//! Mastodon's silence (`limited`) as one local viewer sees it: a silenced
//! account's posts and notifications reach only the local accounts that follow
//! it (`StatusFilter#silenced_account?`, `NotifyService` `from_limited?`).
//! cfwdon silences remote accounts through `silence` domain blocks only; it
//! has no per-account limit.

use crate::db_utils::d1_results;
use crate::federation::instance_domain_blocks;
use crate::tracked_d1::D1Database;
use serde::Deserialize;
use std::collections::HashSet;
use worker::Result;
use worker::d1::D1Type;

/// The actor URIs among `authors` (each an acct domain and an actor URI, which
/// differ under WebFinger delegation) that a domain block silences and that
/// the viewer does not follow; an anonymous viewer follows no one. Reads D1
/// only when one is silenced.
pub(crate) async fn silenced_authors_hidden_from<'a>(
    db: &D1Database,
    viewer_account_id: Option<&str>,
    authors: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<HashSet<String>> {
    let blocks = instance_domain_blocks().await?;
    let silenced = authors
        .into_iter()
        .filter(|(domain, actor_uri)| blocks.restrictions_for_actor(domain, actor_uri).silenced)
        .map(|(_, actor_uri)| actor_uri.to_owned())
        .collect::<HashSet<_>>();
    let Some(viewer_account_id) = viewer_account_id.filter(|_| !silenced.is_empty()) else {
        return Ok(silenced);
    };
    let followed = list_followed_actor_uris_among(db, viewer_account_id, &silenced).await?;
    Ok(silenced
        .into_iter()
        .filter(|actor_uri| !followed.contains(actor_uri))
        .collect())
}

/// The actors among `target_actor_uris` that `follower_account_id` follows
/// (an accepted follow, not a pending request).
async fn list_followed_actor_uris_among(
    db: &D1Database,
    follower_account_id: &str,
    target_actor_uris: &HashSet<String>,
) -> Result<HashSet<String>> {
    #[derive(Deserialize)]
    struct Row {
        target_actor_uri: String,
    }
    let targets = serde_json::to_string(target_actor_uris)
        .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let bindings = [
        D1Type::Text(follower_account_id),
        D1Type::Text(targets.as_str()),
    ];
    let result = db
        .prepare(
            "SELECT target_actor_uri
             FROM follows
             WHERE follower_account_id = ?1
               AND state = 'accepted'
               AND target_actor_uri IN (SELECT value FROM json_each(?2))",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    Ok(d1_results::<Row>(&result)?
        .into_iter()
        .map(|row| row.target_actor_uri)
        .collect())
}
