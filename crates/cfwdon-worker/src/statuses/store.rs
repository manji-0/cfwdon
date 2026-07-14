use super::{
    D1Database, LocalAccount, Result, is_local_follower_authorized,
    is_public_activitypub_visibility,
};
use cfwdon_domain::{LocalStatus, LocalStatusRecord, local_status_default_quote_state};

pub(crate) type StatusRecord = LocalStatusRecord;
pub(crate) type StatusRow = LocalStatus;

pub(crate) fn status_from_record(record: StatusRecord) -> StatusRow {
    LocalStatus::from_record(record)
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

pub(crate) fn status_is_visible_to_requester(
    status: &StatusRow,
    viewer: Option<&LocalAccount>,
    owner: &LocalAccount,
) -> bool {
    is_public_activitypub_visibility(status.visibility.as_str())
        || viewer
            .map(|viewer| viewer.id() == owner.id())
            .unwrap_or(false)
}

pub(crate) async fn can_view_local_status(
    db: &D1Database,
    status: &StatusRow,
    viewer: Option<&LocalAccount>,
    owner: &LocalAccount,
) -> Result<bool> {
    if status_is_visible_to_requester(status, viewer, owner) {
        return Ok(true);
    }
    if status.visibility != cfwdon_domain::Visibility::FollowersOnly {
        return Ok(false);
    }

    let Some(viewer) = viewer else {
        return Ok(false);
    };
    is_local_follower_authorized(db, viewer.id(), owner.id()).await
}
