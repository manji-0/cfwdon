use super::{
    D1Database, LocalAccount, Result, is_local_follower_authorized,
    is_public_activitypub_visibility, local_account_participates_in_direct_status,
};
use cfwdon_domain::{LocalStatus, LocalStatusRecord, Visibility, local_status_default_quote_state};

pub(crate) type StatusRecord = LocalStatusRecord;
pub(crate) type StatusRow = LocalStatus;

pub(crate) fn status_from_record(record: StatusRecord) -> Result<StatusRow> {
    LocalStatus::try_from_record(record)
        .map_err(|error| worker::Error::RustError(error.to_string()))
}

pub(crate) fn statuses_from_records(records: Vec<StatusRecord>) -> Result<Vec<StatusRow>> {
    records.into_iter().map(status_from_record).collect()
}

pub(crate) fn default_quote_state() -> String {
    local_status_default_quote_state()
}

pub(crate) fn effective_status_quote_state(status: &StatusRow) -> &'static str {
    status.effective_quote_state().as_str()
}

pub(crate) fn status_has_active_quote(status: &StatusRow) -> bool {
    status.has_active_quote()
}

/// Mirrors quote revoke API guard: only the quote author may revoke an active quote.
pub(crate) fn local_quote_revoke_allowed(
    requester_account_id: &str,
    quote: &StatusRow,
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
    status: &StatusRow,
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
