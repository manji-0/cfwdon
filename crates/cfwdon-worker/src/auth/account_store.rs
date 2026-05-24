use cfwdon_core::{AppConfig, AuthenticatedUser};
use cfwdon_domain::LocalAccount;
use worker::d1::D1Type;
use worker::{D1Database, Error, Result};

use crate::AccountRow;
use crate::crypto_keys::generate_account_key_material;
use crate::secret_storage::{decrypt_secret, encrypt_secret, is_encrypted_secret};

pub(crate) async fn resolve_local_account(
    db: &D1Database,
    config: &AppConfig,
    user: &AuthenticatedUser,
) -> Result<LocalAccount> {
    if let Some(account) = find_account_by_email(db, &user.email).await? {
        return ensure_account_keys(db, config, account).await;
    }

    let base_username = username_from_email(&user.email);
    let candidate = match find_account_by_username(db, &base_username).await? {
        Some(_) => format!("{}-{}", base_username, short_email_suffix(&user.email)),
        None => base_username,
    };

    let display_name = candidate.clone();
    let key_material = generate_account_key_material().await?;
    let bindings = [
        D1Type::Text(candidate.as_str()),
        D1Type::Text(user.email.as_str()),
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
    store_account_private_key(db, config, &account.id, &key_material.private_key_jwk).await?;
    Ok(account)
}

pub(crate) async fn ensure_account_keys(
    db: &D1Database,
    config: &AppConfig,
    account: LocalAccount,
) -> Result<LocalAccount> {
    if !account.public_key_pem.is_empty()
        && load_account_private_key_jwk(db, config, &account.id)
            .await?
            .is_some()
    {
        return Ok(account);
    }

    let key_material = generate_account_key_material().await?;
    let bindings = [
        D1Type::Text(key_material.private_key_jwk.as_str()),
        D1Type::Text(key_material.public_key_pem.as_str()),
        D1Type::Text(account.id.as_str()),
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
    store_account_private_key(db, config, &account.id, &key_material.private_key_jwk).await?;

    find_account_by_id(db, &account.id)
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

    Ok(row.map(LocalAccount::from))
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

    Ok(row.map(LocalAccount::from))
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

    Ok(row.map(LocalAccount::from))
}

fn username_from_email(email: &str) -> String {
    let local = email.split('@').next().unwrap_or("user");
    let sanitized: String = local
        .chars()
        .map(|ch| ch.to_ascii_lowercase())
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect();

    if sanitized.is_empty() {
        "user".to_owned()
    } else {
        sanitized
    }
}

fn short_email_suffix(email: &str) -> String {
    let checksum = email.bytes().fold(0u32, |acc, byte| {
        acc.wrapping_mul(16777619).wrapping_add(byte as u32)
    });

    format!("{:06x}", checksum & 0x00ff_ffff)
}
