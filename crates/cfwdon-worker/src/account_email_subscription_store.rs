use worker::d1::D1Type;
use worker::{D1Database, Result};

pub(crate) async fn set_account_email_subscription(
    db: &D1Database,
    account_id: &str,
    target_account_id: Option<&str>,
    target_actor_uri: &str,
    enabled: bool,
) -> Result<()> {
    if enabled {
        let bindings = [
            D1Type::Text(account_id),
            target_account_id.map(D1Type::Text).unwrap_or(D1Type::Null),
            D1Type::Text(target_actor_uri),
        ];
        db.prepare(
            "INSERT INTO account_email_subscriptions (
                account_id,
                target_account_id,
                target_actor_uri,
                created_at,
                updated_at
            ) VALUES (
                ?1,
                ?2,
                ?3,
                CURRENT_TIMESTAMP,
                CURRENT_TIMESTAMP
            )
            ON CONFLICT(account_id, target_actor_uri) DO UPDATE SET
                target_account_id = excluded.target_account_id,
                updated_at = CURRENT_TIMESTAMP",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    } else {
        let bindings = [D1Type::Text(account_id), D1Type::Text(target_actor_uri)];
        db.prepare(
            "DELETE FROM account_email_subscriptions
             WHERE account_id = ?1
               AND target_actor_uri = ?2",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}
