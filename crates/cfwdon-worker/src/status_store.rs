use super::{
    D1Database, LocalAccount, Result, is_local_follower_authorized,
    is_public_activitypub_visibility,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct StatusRow {
    pub(crate) id: String,
    pub(crate) account_id: String,
    pub(crate) ap_id: Option<String>,
    pub(crate) in_reply_to_id: Option<String>,
    #[serde(default)]
    pub(crate) boost_of_uri: Option<String>,
    #[serde(default)]
    pub(crate) quote_of_uri: Option<String>,
    pub(crate) content_html: String,
    #[serde(rename = "text_content")]
    pub(crate) _text_content: String,
    pub(crate) spoiler_text: String,
    pub(crate) visibility: String,
    pub(crate) sensitive: i32,
    pub(crate) language: Option<String>,
    #[serde(default)]
    pub(crate) quote_approval_policy: Option<String>,
    #[serde(default = "default_quote_state")]
    pub(crate) quote_state: String,
    #[serde(default)]
    pub(crate) application_id: Option<i64>,
    pub(crate) created_at: String,
    #[serde(default)]
    pub(crate) updated_at: Option<String>,
}

pub(crate) fn default_quote_state() -> String {
    "accepted".to_owned()
}

pub(crate) fn effective_status_quote_state(status: &StatusRow) -> &str {
    if status.quote_of_uri.is_none() {
        "accepted"
    } else {
        status.quote_state.as_str()
    }
}

pub(crate) fn status_has_active_quote(status: &StatusRow) -> bool {
    status.quote_of_uri.is_some() && effective_status_quote_state(status) != "revoked"
}

pub(crate) fn status_is_visible_to_requester(
    status: &StatusRow,
    viewer: Option<&LocalAccount>,
    owner: &LocalAccount,
) -> bool {
    is_public_activitypub_visibility(&status.visibility)
        || viewer.map(|viewer| viewer.id == owner.id).unwrap_or(false)
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
    if status.visibility != "private" {
        return Ok(false);
    }

    let Some(viewer) = viewer else {
        return Ok(false);
    };
    is_local_follower_authorized(db, &viewer.id, &owner.id).await
}
