//! Account signing keys for outbound HTTP signatures. Loading one reads the
//! encrypted JWK from D1, decrypts it and imports it, so deliveries reuse the
//! imported key (non-extractable) within an isolate for a few minutes.

use super::import_account_signing_key;
use crate::auth::load_account_private_key_jwk;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use std::cell::RefCell;
use std::collections::HashMap;
use web_sys::CryptoKey;
use worker::Result;

const ACCOUNT_SIGNING_KEY_L1_TTL_MS: f64 = 5.0 * 60_000.0;
const ACCOUNT_SIGNING_KEY_L1_MAX_ENTRIES: usize = 64;

thread_local! {
    static ACCOUNT_SIGNING_KEY_L1: RefCell<HashMap<String, (CryptoKey, f64)>> =
        RefCell::new(HashMap::new());
}

fn l1_get(account_id: &str, now_ms: f64) -> Option<CryptoKey> {
    ACCOUNT_SIGNING_KEY_L1.with(|cache| {
        cache
            .borrow()
            .get(account_id)
            .filter(|(_, expires_at_ms)| *expires_at_ms > now_ms)
            .map(|(key, _)| key.clone())
    })
}

fn l1_put(account_id: &str, key: CryptoKey, now_ms: f64) {
    ACCOUNT_SIGNING_KEY_L1.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.retain(|_, (_, expires_at_ms)| *expires_at_ms > now_ms);
        if cache.len() >= ACCOUNT_SIGNING_KEY_L1_MAX_ENTRIES {
            cache.clear();
        }
        cache.insert(
            account_id.to_owned(),
            (key, now_ms + ACCOUNT_SIGNING_KEY_L1_TTL_MS),
        );
    });
}

pub(crate) async fn load_account_signing_key(
    db: &D1Database,
    config: &AppConfig,
    account_id: &str,
) -> Result<Option<CryptoKey>> {
    let now_ms = js_sys::Date::now();
    if let Some(key) = l1_get(account_id, now_ms) {
        return Ok(Some(key));
    }
    let Some(private_key_jwk) = load_account_private_key_jwk(db, config, account_id).await? else {
        return Ok(None);
    };
    let key = import_account_signing_key(&private_key_jwk).await?;
    l1_put(account_id, key.clone(), now_ms);
    Ok(Some(key))
}
