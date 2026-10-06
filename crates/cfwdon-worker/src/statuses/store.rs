use super::{D1Database, LocalAccount};
use crate::activitypub::is_public_activitypub_visibility;
use crate::conversation_store::local_account_participates_in_direct_status;
use crate::relationship::is_local_follower_authorized;
use cfwdon_domain::{
    LocalStatus, LocalStatusRecord, RemoteStatus, Visibility, local_status_default_quote_state,
};
use worker::Result;
use worker::d1::D1Type;

pub(crate) fn status_from_record(record: LocalStatusRecord) -> Result<LocalStatus> {
    LocalStatus::try_from_record(record)
        .map_err(|error| worker::Error::RustError(error.to_string()))
}

pub(crate) fn statuses_from_records(records: Vec<LocalStatusRecord>) -> Result<Vec<LocalStatus>> {
    records.into_iter().map(status_from_record).collect()
}

pub(crate) fn default_quote_state() -> String {
    local_status_default_quote_state()
}

pub(crate) fn effective_status_quote_state(status: &LocalStatus) -> &'static str {
    status.effective_quote_state().as_str()
}

pub(crate) fn status_has_active_quote(status: &LocalStatus) -> bool {
    status.has_active_quote()
}

/// Mirrors quote revoke API guard: only the quote author may revoke an active quote.
pub(crate) fn local_quote_revoke_allowed(
    requester_account_id: &str,
    quote: &LocalStatus,
    target_uri: &str,
) -> bool {
    quote.account_id == requester_account_id
        && quote.quote_of_uri.as_deref() == Some(target_uri)
        && status_has_active_quote(quote)
}

/// Pure visibility matrix used by [`can_view_local_status`] after relationship lookups.
pub(crate) fn local_status_allows_viewer(
    visibility: Visibility,
    is_owner: bool,
    is_follower: bool,
    is_direct_participant: bool,
) -> bool {
    match visibility {
        Visibility::Public | Visibility::Unlisted => true,
        Visibility::FollowersOnly => is_owner || is_follower,
        Visibility::Direct => is_owner || is_direct_participant,
    }
}

pub(crate) async fn can_view_local_status(
    db: &D1Database,
    status: &LocalStatus,
    viewer: Option<&LocalAccount>,
    owner: &LocalAccount,
) -> Result<bool> {
    if is_public_activitypub_visibility(status.visibility.as_str()) {
        return Ok(true);
    }

    let Some(viewer) = viewer else {
        return Ok(false);
    };
    if viewer.id() == owner.id() {
        return Ok(true);
    }

    match status.visibility {
        Visibility::FollowersOnly => Ok(local_status_allows_viewer(
            status.visibility,
            false,
            is_local_follower_authorized(db, viewer.id(), owner.id()).await?,
            false,
        )),
        Visibility::Direct => Ok(local_status_allows_viewer(
            status.visibility,
            false,
            false,
            local_account_participates_in_direct_status(db, &status.id, viewer.id()).await?,
        )),
        // Public/unlisted already returned true above.
        Visibility::Public | Visibility::Unlisted => Ok(true),
    }
}

/// Whether `viewer` may see a stored remote status. Followers-only posts are
/// visible to accepted followers of the author and to mentioned accounts;
/// direct posts only to mentioned accounts, as in Mastodon's `StatusPolicy`.
pub(crate) async fn can_view_remote_status(
    db: &D1Database,
    status: &RemoteStatus,
    viewer: Option<&LocalAccount>,
) -> Result<bool> {
    if is_public_activitypub_visibility(status.visibility.as_str()) {
        return Ok(true);
    }
    let Some(viewer) = viewer else {
        return Ok(false);
    };
    let followers_only = i32::from(status.visibility == Visibility::FollowersOnly);
    let bindings = [
        D1Type::Text(status.id.as_str()),
        D1Type::Text(viewer.id()),
        D1Type::Integer(followers_only),
        D1Type::Text(status.actor_uri.as_str()),
    ];
    let row = db
        .prepare(
            "SELECT 1 AS found
             WHERE EXISTS (
                     SELECT 1 FROM remote_status_mentions
                     WHERE status_id = ?1
                       AND account_id = ?2
                 )
                OR (
                     ?3 = 1
                     AND EXISTS (
                         SELECT 1 FROM follows
                         WHERE follower_account_id = ?2
                           AND target_actor_uri = ?4
                           AND state = 'accepted'
                     )
                 )",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;
    Ok(row.is_some())
}
