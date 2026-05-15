use crate::{
    D1Database, Result, set_account_email_subscription, set_account_endorsement, set_account_note,
};

pub(crate) async fn set_relationship_endorsement(
    db: &D1Database,
    viewer_id: &str,
    target_account_id: Option<&str>,
    target_actor_uri: &str,
    endorsed: bool,
) -> Result<()> {
    set_account_endorsement(db, viewer_id, target_account_id, target_actor_uri, endorsed).await
}

pub(crate) async fn set_relationship_note(
    db: &D1Database,
    viewer_id: &str,
    target_account_id: Option<&str>,
    target_actor_uri: &str,
    note: &str,
) -> Result<()> {
    set_account_note(db, viewer_id, target_account_id, target_actor_uri, note).await
}

pub(crate) async fn set_relationship_email_subscription(
    db: &D1Database,
    viewer_id: &str,
    target_account_id: Option<&str>,
    target_actor_uri: &str,
    enabled: bool,
) -> Result<()> {
    set_account_email_subscription(db, viewer_id, target_account_id, target_actor_uri, enabled)
        .await
}
