use cfwdon_core::AuthenticatedUser;
use cfwdon_domain::LocalAccount;
use worker::d1::D1Type;
use worker::{D1Database, Error, Result};

use super::crypto_keys::generate_account_key_material;
use crate::AccountRow;

pub(crate) async fn resolve_local_account(
    db: &D1Database,
    user: &AuthenticatedUser,
) -> Result<LocalAccount> {
    if let Some(account) = find_account_by_email(db, &user.email).await? {
        return ensure_account_keys(db, account).await;
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
        D1Type::Text(key_material.private_key_jwk.as_str()),
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

    find_account_by_email(db, &user.email)
        .await?
        .ok_or_else(|| Error::RustError("failed to load provisioned account".to_owned()))
}

pub(crate) async fn ensure_account_keys(
    db: &D1Database,
    account: LocalAccount,
) -> Result<LocalAccount> {
    if !account.private_key_jwk.is_empty() && !account.public_key_pem.is_empty() {
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
         SET private_key_jwk = ?1,
             public_key_pem = ?2,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    find_account_by_id(db, &account.id)
        .await?
        .ok_or_else(|| Error::RustError("failed to reload account key material".to_owned()))
}

pub(crate) async fn find_account_by_email(
    db: &D1Database,
    email: &str,
) -> Result<Option<LocalAccount>> {
    let email = D1Type::Text(email);

    let row = db
        .prepare(
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, private_key_jwk, public_key_pem, created_at
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
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, private_key_jwk, public_key_pem, created_at
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
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, private_key_jwk, public_key_pem, created_at
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
