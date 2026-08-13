use cfwdon_core::{AppConfig, AuthenticatedUser};
use cfwdon_domain::{ComposingAccessProvision, LocalAccount};
use worker::d1::D1Type;
use worker::{Error, Result};

use crate::AccountRow;
use crate::crypto_keys::generate_account_key_material;
use crate::secret_storage::{decrypt_secret, encrypt_secret, is_encrypted_secret};

use crate::D1Database;
pub(crate) async fn resolve_local_account(
    db: &D1Database,
    config: &AppConfig,
    user: &AuthenticatedUser,
) -> Result<LocalAccount> {
    if let Some(account) = find_account_by_email(db, &user.email).await? {
        return ensure_account_keys(db, config, account).await;
    }

    let base_username_taken = {
        let email = cfwdon_domain::AccessEmail::parse(&user.email).map_err(|error| {
            Error::RustError(format!("authenticated user email is invalid: {error}"))
        })?;
        let base = cfwdon_domain::Username::derive_from_email(&email, false).map_err(|error| {
            Error::RustError(format!("authenticated user email is invalid: {error}"))
        })?;
        find_account_by_username(db, base.as_str()).await?.is_some()
    };
    let provision = ComposingAccessProvision {
        email: user.email.clone(),
    }
    .resolve(base_username_taken)
    .map_err(|error| Error::RustError(format!("authenticated user email is invalid: {error}")))?;
    let candidate = provision.username.as_str();
    let display_name = candidate.to_owned();
    let key_material = generate_account_key_material().await?;
    let bindings = [
        D1Type::Text(candidate),
        D1Type::Text(provision.email.as_str()),
        D1Type::Text(display_name.as_str()),
        D1Type::Text(""),
        D1Type::Text(key_material.public_key_pem.as_str()),
    ];

    db.prepare(
        "INSERT INTO accounts (
            id,
            username,
            access_email,
            display_name,
            fields_json,
            discoverable,
            default_quote_policy,
            private_key_jwk,
            public_key_pem,
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            ?2,
            ?3,
            '[]',
            0,
            'public',
            ?4,
            ?5,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let account = find_account_by_email(db, &user.email)
        .await?
        .ok_or_else(|| Error::RustError("failed to load provisioned account".to_owned()))?;
    store_account_private_key(db, config, account.id(), &key_material.private_key_jwk).await?;
    Ok(account)
}

pub(crate) async fn ensure_account_keys(
    db: &D1Database,
    config: &AppConfig,
    account: LocalAccount,
) -> Result<LocalAccount> {
    if !account.public_key_pem().is_empty()
        && load_account_private_key_jwk(db, config, account.id())
            .await?
            .is_some()
    {
        return Ok(account);
    }

    let key_material = generate_account_key_material().await?;
    let bindings = [
        D1Type::Text(key_material.private_key_jwk.as_str()),
        D1Type::Text(key_material.public_key_pem.as_str()),
        D1Type::Text(account.id()),
    ];

    db.prepare(
        "UPDATE accounts
         SET private_key_jwk = '',
             public_key_pem = ?2,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    store_account_private_key(db, config, account.id(), &key_material.private_key_jwk).await?;

    find_account_by_id(db, account.id())
        .await?
        .ok_or_else(|| Error::RustError("failed to reload account key material".to_owned()))
}

pub(crate) async fn store_account_private_key(
    db: &D1Database,
    config: &AppConfig,
    account_id: &str,
    private_key_jwk: &str,
) -> Result<()> {
    if let Some(encryption_key) = config.account_private_key_encryption_key.as_deref() {
        let encrypted = if is_encrypted_secret(private_key_jwk) {
            private_key_jwk.to_owned()
        } else {
            encrypt_secret(private_key_jwk, encryption_key).await?
        };
        let bindings = [
            D1Type::Text(account_id),
            D1Type::Text(encrypted.as_str()),
            D1Type::Text(account_id),
        ];
        db.prepare(
            "INSERT INTO account_private_keys (
                account_id,
                private_key_jwk_encrypted,
                created_at,
                updated_at
            ) VALUES (
                ?1,
                ?2,
                CURRENT_TIMESTAMP,
                CURRENT_TIMESTAMP
            )
            ON CONFLICT(account_id) DO UPDATE SET
                private_key_jwk_encrypted = excluded.private_key_jwk_encrypted,
                updated_at = CURRENT_TIMESTAMP",
        )
        .bind_refs(bindings[..2].iter())?
        .run()
        .await?;
        db.prepare(
            "UPDATE accounts
             SET private_key_jwk = '',
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1",
        )
        .bind_refs(&bindings[2])?
        .run()
        .await?;
        return Ok(());
    }

    let bindings = [D1Type::Text(private_key_jwk), D1Type::Text(account_id)];
    db.prepare(
        "UPDATE accounts
         SET private_key_jwk = ?1,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn load_account_private_key_jwk(
    db: &D1Database,
    config: &AppConfig,
    account_id: &str,
) -> Result<Option<String>> {
    let account_id_binding = D1Type::Text(account_id);
    if let Some(row) = db
        .prepare(
            "SELECT private_key_jwk_encrypted
             FROM account_private_keys
             WHERE account_id = ?1
             LIMIT 1",
        )
        .bind_refs(&account_id_binding)?
        .first::<serde_json::Value>(None)
        .await?
    {
        let Some(encrypted) = row
            .get("private_key_jwk_encrypted")
            .and_then(serde_json::Value::as_str)
        else {
            return Ok(None);
        };
        let Some(encryption_key) = config.account_private_key_encryption_key.as_deref() else {
            return Err(Error::RustError(
                "ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY is required to load encrypted account keys"
                    .to_owned(),
            ));
        };
        return decrypt_secret(encrypted, encryption_key).await.map(Some);
    }

    let Some(row) = db
        .prepare(
            "SELECT private_key_jwk
             FROM accounts
             WHERE id = ?1
             LIMIT 1",
        )
        .bind_refs(&account_id_binding)?
        .first::<serde_json::Value>(None)
        .await?
    else {
        return Ok(None);
    };
    let legacy_private_key = row
        .get("private_key_jwk")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(legacy_private_key) = legacy_private_key {
        if config.account_private_key_encryption_key.is_some() {
            store_account_private_key(db, config, account_id, legacy_private_key).await?;
        }
        return Ok(Some(legacy_private_key.to_owned()));
    }
    Ok(None)
}

pub(crate) async fn find_account_by_email(
    db: &D1Database,
    email: &str,
) -> Result<Option<LocalAccount>> {
    let email = D1Type::Text(email);

    let row = db
        .prepare(
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, '' AS private_key_jwk, public_key_pem, created_at
             FROM accounts
             WHERE access_email = ?1
             LIMIT 1",
        )
        .bind_refs(&email)?
        .first::<AccountRow>(None)
        .await?;

    Ok(row.map(LocalAccount::from_record))
}

pub(crate) async fn find_account_by_id(db: &D1Database, id: &str) -> Result<Option<LocalAccount>> {
    let id = D1Type::Text(id);

    let row = db
        .prepare(
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, '' AS private_key_jwk, public_key_pem, created_at
             FROM accounts
             WHERE id = ?1
             LIMIT 1",
        )
        .bind_refs(&id)?
        .first::<AccountRow>(None)
        .await?;

    Ok(row.map(LocalAccount::from_record))
}

pub(crate) async fn find_account_by_username(
    db: &D1Database,
    username: &str,
) -> Result<Option<LocalAccount>> {
    let username = username.trim().to_ascii_lowercase();
    let username = D1Type::Text(username.as_str());

    let row = db
        .prepare(
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, '' AS private_key_jwk, public_key_pem, created_at
             FROM accounts
             WHERE username = ?1
             LIMIT 1",
        )
        .bind_refs(&username)?
        .first::<AccountRow>(None)
        .await?;

    Ok(row.map(LocalAccount::from_record))
}

/// Prefer a stable human account for signed ActivityPub GETs when no viewer is
/// available (inbox Announce hydration, background `remote_context_fetch`, etc.).
/// Test/smoke usernames are deprioritized so we do not advertise disposable keyIds.
pub(crate) async fn find_any_local_account(db: &D1Database) -> Result<Option<LocalAccount>> {
    let row = db
        .prepare(
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, '' AS private_key_jwk, public_key_pem, created_at
             FROM accounts
             ORDER BY
               CASE
                 WHEN username LIKE 'codex_smoke_%' THEN 2
                 WHEN username LIKE 'cfclient%' THEN 2
                 WHEN username LIKE 'phanpy%' THEN 2
                 WHEN username LIKE 'accessflow%' THEN 2
                 ELSE 0
               END ASC,
               created_at ASC
             LIMIT 1",
        )
        .first::<AccountRow>(None)
        .await?;

    Ok(row.map(LocalAccount::from_record))
}
