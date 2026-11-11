//! The instance actor's key pair (`instance_actor_keys`, one row). It is
//! generated on first use; two first uses that race both read back the pair
//! that was stored first.

use crate::crypto_keys::generate_account_key_material;
use crate::secret_storage::{decrypt_secret, encrypt_secret, is_encrypted_secret};
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use worker::d1::D1Type;
use worker::{Error, Result};

pub(crate) struct InstanceActorKeys {
    pub(crate) public_key_pem: String,
    pub(crate) private_key_jwk: String,
}

impl std::fmt::Debug for InstanceActorKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstanceActorKeys")
            .field("public_key_pem", &self.public_key_pem)
            .field("private_key_jwk", &"<redacted>")
            .finish()
    }
}

#[derive(serde::Deserialize)]
struct InstanceActorKeysRow {
    public_key_pem: String,
    private_key_jwk: String,
}

pub(crate) async fn instance_actor_keys(
    db: &D1Database,
    config: &AppConfig,
) -> Result<InstanceActorKeys> {
    if let Some(row) = find_instance_actor_keys_row(db).await? {
        return open_instance_actor_keys(db, config, row).await;
    }

    let generated = generate_account_key_material().await?;
    let stored_private_key = seal_private_key(config, &generated.private_key_jwk).await?;
    let bindings = [
        D1Type::Text(generated.public_key_pem.as_str()),
        D1Type::Text(stored_private_key.as_str()),
    ];
    db.prepare(
        "INSERT INTO instance_actor_keys (id, public_key_pem, private_key_jwk, created_at)
         VALUES (1, ?1, ?2, CURRENT_TIMESTAMP)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let row = find_instance_actor_keys_row(db)
        .await?
        .ok_or_else(|| Error::RustError("instance actor keys missing after insert".to_owned()))?;
    open_instance_actor_keys(db, config, row).await
}

async fn find_instance_actor_keys_row(db: &D1Database) -> Result<Option<InstanceActorKeysRow>> {
    db.prepare("SELECT public_key_pem, private_key_jwk FROM instance_actor_keys WHERE id = 1")
        .first::<InstanceActorKeysRow>(None)
        .await
}

async fn seal_private_key(config: &AppConfig, private_key_jwk: &str) -> Result<String> {
    match config.account_private_key_encryption_key.as_deref() {
        Some(encryption_key) => encrypt_secret(private_key_jwk, encryption_key).await,
        None => Ok(private_key_jwk.to_owned()),
    }
}

/// Decrypts the stored private key. A key stored in plaintext before
/// ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY was configured is encrypted in place.
async fn open_instance_actor_keys(
    db: &D1Database,
    config: &AppConfig,
    row: InstanceActorKeysRow,
) -> Result<InstanceActorKeys> {
    let encryption_key = config.account_private_key_encryption_key.as_deref();
    let private_key_jwk = if is_encrypted_secret(&row.private_key_jwk) {
        let encryption_key = encryption_key.ok_or_else(|| {
            Error::RustError(
                "ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY is required to load the instance actor key"
                    .to_owned(),
            )
        })?;
        decrypt_secret(&row.private_key_jwk, encryption_key).await?
    } else {
        if let Some(encryption_key) = encryption_key {
            let encrypted = encrypt_secret(&row.private_key_jwk, encryption_key).await?;
            db.prepare("UPDATE instance_actor_keys SET private_key_jwk = ?1 WHERE id = 1")
                .bind_refs(&D1Type::Text(encrypted.as_str()))?
                .run()
                .await?;
        }
        row.private_key_jwk
    };
    Ok(InstanceActorKeys {
        public_key_pem: row.public_key_pem,
        private_key_jwk,
    })
}
