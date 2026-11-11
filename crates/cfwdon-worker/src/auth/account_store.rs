use crate::crypto_keys::generate_account_key_material;
use crate::secret_storage::{decrypt_secret, encrypt_secret, is_encrypted_secret};
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::{LocalAccount, LocalAccountRecord};
use worker::d1::D1Type;
use worker::{Error, Result};
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

    find_account_by_id_with(db, account.id(), AccountLookup::IncludingPending)
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

/// The account holding `email`, pending or not: registration and e-mail
/// changes keep an address unique across every account, and an Auth0 sign-in
/// needs some account to sign in to. Only the id comes
/// back, so a caller cannot act as the account; acting on an e-mail's account
/// goes through `find_credential_account_by_email`.
pub(crate) async fn find_account_id_holding_email(
    db: &D1Database,
    email: &str,
) -> Result<Option<String>> {
    #[derive(serde::Deserialize)]
    struct Row {
        id: String,
    }
    Ok(db
        .prepare("SELECT id FROM accounts WHERE access_email = ?1 LIMIT 1")
        .bind_refs(&D1Type::Text(email))?
        .first::<Row>(None)
        .await?
        .map(|row| row.id))
}

const ACCOUNT_SELECT: &str = "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, '' AS private_key_jwk, public_key_pem, created_at
     FROM accounts";

/// Mastodon's `check_account_approval`: an account waiting for approval does
/// not exist for anyone but its own credentials, the admin review, and
/// uniqueness checks.
pub(crate) const APPROVED_ONLY: &str = "NOT EXISTS (
         SELECT 1 FROM pending_account_approvals p WHERE p.account_id = accounts.id
     )";

/// Which accounts a lookup by id or username may return.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccountLookup {
    /// Accounts their owners may use; what every public surface shows.
    Approved,
    /// Pending accounts too, for the admin review, sign-up uniqueness, and
    /// handing a pending account its token (Mastodon issues those).
    IncludingPending,
}

/// The columns an account lookup may key on. `access_email` is not one: an
/// e-mail resolves to an account only through
/// `find_credential_account_by_email`, which returns the approval with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AccountKey {
    Id,
    Username,
}

impl AccountKey {
    fn column(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Username => "username",
        }
    }
}

async fn find_account_where(
    db: &D1Database,
    key: AccountKey,
    value: &str,
    lookup: AccountLookup,
) -> Result<Option<LocalAccount>> {
    let column = key.column();
    let approval_filter = match lookup {
        AccountLookup::Approved => format!(" AND {APPROVED_ONLY}"),
        AccountLookup::IncludingPending => String::new(),
    };
    let row = db
        .prepare(format!(
            "{ACCOUNT_SELECT}
             WHERE {column} = ?1{approval_filter}
             LIMIT 1"
        ))
        .bind_refs(&D1Type::Text(value))?
        .first::<LocalAccountRecord>(None)
        .await?;
    Ok(row.map(LocalAccount::from_record))
}

/// An approved account by id; a pending one is not found.
pub(crate) async fn find_account_by_id(db: &D1Database, id: &str) -> Result<Option<LocalAccount>> {
    find_account_where(db, AccountKey::Id, id, AccountLookup::Approved).await
}

/// An approved account by username; a pending one is not found.
pub(crate) async fn find_account_by_username(
    db: &D1Database,
    username: &str,
) -> Result<Option<LocalAccount>> {
    find_account_by_username_with(db, username, AccountLookup::Approved).await
}

pub(crate) async fn find_account_by_id_with(
    db: &D1Database,
    id: &str,
    lookup: AccountLookup,
) -> Result<Option<LocalAccount>> {
    find_account_where(db, AccountKey::Id, id, lookup).await
}

pub(crate) async fn find_account_by_username_with(
    db: &D1Database,
    username: &str,
    lookup: AccountLookup,
) -> Result<Option<LocalAccount>> {
    let username = username.trim().to_ascii_lowercase();
    find_account_where(db, AccountKey::Username, &username, lookup).await
}
