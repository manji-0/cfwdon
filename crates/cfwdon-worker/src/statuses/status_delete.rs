use super::{
    StatusRow, enqueue_addressed_delete_activity, enqueue_direct_delete_activity,
    outbox_delete_insert_statement, reblog_wrapper_status_target_bindings,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::Result;
use worker::d1::D1Type;

use crate::D1Database;

pub(crate) async fn delete_reblog_wrapper_status_by_target_uri(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<()> {
    let bindings = reblog_wrapper_status_target_bindings(account_id, target_uri);
    db.prepare(
        "DELETE FROM statuses
         WHERE account_id = ?1
           AND boost_of_uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn delete_status_poll(db: &D1Database, status_id: &str) -> Result<()> {
    let bindings = status_id_delete_bindings(status_id);
    db.prepare(
        "DELETE FROM status_poll_options
         WHERE poll_id IN (
             SELECT id
             FROM status_polls
             WHERE status_id = ?1
         )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let bindings = status_id_delete_bindings(status_id);
    db.prepare(
        "DELETE FROM status_polls
         WHERE status_id = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

fn status_id_delete_bindings(status_id: &str) -> [D1Type<'_>; 1] {
    [D1Type::Text(status_id)]
}

pub(crate) async fn delete_local_status_with_outbox(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<()> {
    enqueue_direct_delete_activity(db, config, account, status).await?;
    enqueue_addressed_delete_activity(db, config, account, status).await?;

    let mut statements = Vec::new();
    if let Some(statement) = outbox_delete_insert_statement(db, config, account, status).await? {
        statements.push(statement);
    }

    let bindings = status_id_delete_bindings(&status.id);
    statements.push(
        db.prepare(
            "DELETE FROM status_poll_options
             WHERE poll_id IN (
                 SELECT id
                 FROM status_polls
                 WHERE status_id = ?1
             )",
        )
        .bind_refs(bindings.iter())?,
    );
    statements.push(
        db.prepare(
            "DELETE FROM status_polls
             WHERE status_id = ?1",
        )
        .bind_refs(bindings.iter())?,
    );
    statements.push(
        db.prepare(
            "DELETE FROM statuses
             WHERE id = ?1",
        )
        .bind_refs(bindings.iter())?,
    );

    db.batch(statements).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_id_delete_bindings_keep_sql_slot_order_stable() {
        let bindings = status_id_delete_bindings("status-1");

        assert!(matches!(bindings[0], D1Type::Text("status-1")));
    }
}
