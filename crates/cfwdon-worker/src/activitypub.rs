#[allow(unused_imports)]
pub(crate) use crate::*;

mod actor_document;
mod local_uri;
mod objects;
mod parse;
mod social_activities;
mod updates;
pub(crate) use actor_document::*;
pub(crate) use local_uri::*;
pub(crate) use objects::*;
pub(crate) use parse::*;
pub(crate) use social_activities::*;
pub(crate) use updates::*;

use cfwdon_core::AppConfig;
use worker::{D1Database, Result};

pub(crate) async fn build_activitypub_delete(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<serde_json::Value> {
    build_activitypub_delete_with_published_at(db, config, account, status, &now_iso_string()?)
        .await
}

pub(crate) async fn build_activitypub_delete_with_published_at(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
    published_at: &str,
) -> Result<serde_json::Value> {
    let note_id = local_status_ap_id(config, account, status);
    let audiences = activitypub_audiences_for_status(db, config, account, status).await?;
    Ok(serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "type": "Delete",
        "id": format!("{note_id}#delete"),
        "actor": actor_url(config, account.username()),
        "published": published_at,
        "to": audiences.0,
        "cc": audiences.1,
        "object": note_id,
    }))
}
