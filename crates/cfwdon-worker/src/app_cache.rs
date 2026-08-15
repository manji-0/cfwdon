use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use worker::d1::D1Type;
use worker::kv::KvStore;
use worker::{Env, Result};

const ACCOUNT_CAPABILITIES_KEY_PREFIX: &str = "acctcap:v1:";
const ACCOUNT_CAPABILITIES_TTL_SECS: u64 = 3_600;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct AccountCapabilities {
    pub(crate) has_thread_mutes: bool,
    pub(crate) has_followed_tags: bool,
    pub(crate) has_filters: bool,
    pub(crate) has_domain_blocks: bool,
}

thread_local! {
    static APP_CACHE_KV: RefCell<Option<KvStore>> = const { RefCell::new(None) };
    static ACCOUNT_CAPABILITIES_L1: RefCell<HashMap<String, AccountCapabilities>> =
        RefCell::new(HashMap::new());
}

/// Install the optional KV binding used for short-lived app cache entries.
///
/// Missing bindings are ignored so local/unit paths keep working without KV.
pub(crate) fn install_app_cache(env: &Env, binding: &str) {
    let kv = env.kv(binding).ok();
    APP_CACHE_KV.with(|slot| {
        *slot.borrow_mut() = kv;
    });
}

/// Drop request-scoped L1 entries so isolate reuse cannot serve stale bits.
pub(crate) fn reset_app_cache_request_state() {
    ACCOUNT_CAPABILITIES_L1.with(|cache| cache.borrow_mut().clear());
}

pub(crate) fn app_cache_kv() -> Option<KvStore> {
    APP_CACHE_KV.with(|slot| slot.borrow().clone())
}

fn account_capabilities_key(account_id: &str) -> String {
    format!("{ACCOUNT_CAPABILITIES_KEY_PREFIX}{account_id}")
}

fn l1_get(account_id: &str) -> Option<AccountCapabilities> {
    ACCOUNT_CAPABILITIES_L1.with(|cache| cache.borrow().get(account_id).copied())
}

fn l1_put(account_id: &str, caps: AccountCapabilities) {
    ACCOUNT_CAPABILITIES_L1.with(|cache| {
        cache.borrow_mut().insert(account_id.to_owned(), caps);
    });
}

fn l1_remove(account_id: &str) {
    ACCOUNT_CAPABILITIES_L1.with(|cache| {
        cache.borrow_mut().remove(account_id);
    });
}

async fn kv_get_account_capabilities(account_id: &str) -> Option<AccountCapabilities> {
    let kv = app_cache_kv()?;
    let text = kv
        .get(&account_capabilities_key(account_id))
        .text()
        .await
        .ok()??;
    serde_json::from_str(&text).ok()
}

async fn kv_put_account_capabilities(account_id: &str, caps: AccountCapabilities) {
    let Some(kv) = app_cache_kv() else {
        return;
    };
    let body = match serde_json::to_string(&caps) {
        Ok(body) => body,
        Err(_) => return,
    };
    let Ok(putter) = kv.put(&account_capabilities_key(account_id), body) else {
        return;
    };
    let _ = putter
        .expiration_ttl(ACCOUNT_CAPABILITIES_TTL_SECS)
        .execute()
        .await;
}

async fn kv_delete_account_capabilities(account_id: &str) {
    let Some(kv) = app_cache_kv() else {
        return;
    };
    let _ = kv.delete(&account_capabilities_key(account_id)).await;
}

async fn account_has_any_row(db: &crate::D1Database, account_id: &str, sql: &str) -> Result<bool> {
    let account_id = D1Type::Text(account_id);
    Ok(db
        .prepare(sql)
        .bind_refs(&account_id)?
        .first::<serde_json::Value>(None)
        .await?
        .is_some())
}

async fn probe_account_capabilities(
    db: &crate::D1Database,
    account_id: &str,
) -> Result<AccountCapabilities> {
    let (has_thread_mutes, has_followed_tags, has_filters, has_domain_blocks) = futures_util::try_join!(
        account_has_any_row(
            db,
            account_id,
            "SELECT thread_root_status_id
             FROM thread_mutes
             WHERE account_id = ?1
             LIMIT 1",
        ),
        account_has_any_row(
            db,
            account_id,
            "SELECT tag_name
             FROM followed_tags
             WHERE account_id = ?1
             LIMIT 1",
        ),
        account_has_any_row(
            db,
            account_id,
            "SELECT id
             FROM filters
             WHERE account_id = ?1
             LIMIT 1",
        ),
        account_has_any_row(
            db,
            account_id,
            "SELECT domain
             FROM account_domain_blocks
             WHERE account_id = ?1
             LIMIT 1",
        ),
    )?;
    Ok(AccountCapabilities {
        has_thread_mutes,
        has_followed_tags,
        has_filters,
        has_domain_blocks,
    })
}

/// Load cached account capability bits, probing D1 once on miss.
pub(crate) async fn load_account_capabilities(
    db: &crate::D1Database,
    account_id: &str,
) -> Result<AccountCapabilities> {
    if let Some(caps) = l1_get(account_id) {
        return Ok(caps);
    }
    if let Some(caps) = kv_get_account_capabilities(account_id).await {
        l1_put(account_id, caps);
        return Ok(caps);
    }

    let caps = probe_account_capabilities(db, account_id).await?;
    l1_put(account_id, caps);
    kv_put_account_capabilities(account_id, caps).await;
    Ok(caps)
}

/// Drop cached capability bits after a mutation that may change them.
pub(crate) async fn invalidate_account_capabilities(account_id: &str) {
    l1_remove(account_id);
    kv_delete_account_capabilities(account_id).await;
}
