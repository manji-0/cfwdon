//! Admin instance domain blocks (`instance_domain_blocks`), enforced as
//! Mastodon `suspend` blocks: no inbound activities, outbound deliveries, or
//! remote fetches for a blocked domain or any of its subdomains.

use crate::db_utils::d1_results;
use crate::tracked_d1::D1Database;
use cfwdon_domain::host_within_any_blocked_domain;
use serde::Deserialize;
use std::cell::RefCell;
use std::rc::Rc;
use url::Url;
use worker::{Env, Error, Result};

/// Other isolates pick up admin block edits within this window.
const SUSPENDED_DOMAINS_L1_TTL_MS: f64 = 60_000.0;

thread_local! {
    static SUSPENDED_DOMAINS_DB: RefCell<Option<D1Database>> = const { RefCell::new(None) };
    /// `(loaded_at_ms, domains)`; every federation request and fetch reads it.
    static SUSPENDED_DOMAINS_L1: RefCell<Option<(f64, Rc<[String]>)>> =
        const { RefCell::new(None) };
}

/// Install the D1 binding the suspended-domain set loads from. Missing
/// bindings leave enforcement off, so unit paths without D1 keep working.
pub(crate) fn install_domain_suspensions(env: &Env, database_binding: &str) {
    let db = env.d1(database_binding).ok().map(D1Database::new);
    SUSPENDED_DOMAINS_DB.with(|slot| *slot.borrow_mut() = db);
}

fn suspended_domains_from_l1(now_ms: f64) -> Option<Rc<[String]>> {
    SUSPENDED_DOMAINS_L1.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|(loaded_at_ms, _)| now_ms - loaded_at_ms < SUSPENDED_DOMAINS_L1_TTL_MS)
            .map(|(_, domains)| Rc::clone(domains))
    })
}

fn store_suspended_domains_l1(now_ms: f64, domains: Rc<[String]>) {
    SUSPENDED_DOMAINS_L1.with(|slot| *slot.borrow_mut() = Some((now_ms, domains)));
}

/// Drop this isolate's cached set after an admin adds or removes a block.
pub(crate) fn invalidate_suspended_domains() {
    SUSPENDED_DOMAINS_L1.with(|slot| *slot.borrow_mut() = None);
}

#[derive(Deserialize)]
struct DomainRow {
    domain: String,
}

/// Every suspended domain, from this isolate's cache or D1.
pub(crate) async fn suspended_instance_domains() -> Result<Rc<[String]>> {
    let Some(db) =
        SUSPENDED_DOMAINS_DB.with(|slot| slot.borrow().as_ref().map(D1Database::detached))
    else {
        return Ok(Rc::from([]));
    };
    let now_ms = js_sys::Date::now();
    if let Some(domains) = suspended_domains_from_l1(now_ms) {
        return Ok(domains);
    }
    let result = db
        .prepare("SELECT domain FROM instance_domain_blocks ORDER BY domain ASC")
        .all()
        .await?;
    let domains: Rc<[String]> = d1_results::<DomainRow>(&result)?
        .into_iter()
        .map(|row| row.domain)
        .collect();
    store_suspended_domains_l1(now_ms, Rc::clone(&domains));
    Ok(domains)
}

pub(crate) fn host_within_blocked_domains(host: &str, blocked_domains: &[String]) -> bool {
    host_within_any_blocked_domain(host, blocked_domains.iter().map(String::as_str))
}

/// Whether `url`'s host falls under any of `blocked_domains`. Unparseable URLs
/// are left to the caller's own URL validation.
pub(crate) fn url_host_within_blocked_domains(url: &str, blocked_domains: &[String]) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host_within_blocked_domains(host, blocked_domains))
        })
        .unwrap_or(false)
}

/// Refuse a remote fetch or delivery to a suspended domain.
pub(crate) async fn ensure_remote_host_not_suspended(host: &str) -> Result<()> {
    if host_within_blocked_domains(host, &suspended_instance_domains().await?) {
        return Err(Error::RustError(format!(
            "remote host {host} is suspended by an instance domain block"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        SUSPENDED_DOMAINS_L1_TTL_MS, invalidate_suspended_domains, store_suspended_domains_l1,
        suspended_domains_from_l1, url_host_within_blocked_domains,
    };
    use std::rc::Rc;

    #[test]
    fn suspended_domains_l1_expires_and_invalidates() {
        store_suspended_domains_l1(1_000.0, Rc::from(["blocked.example".to_owned()]));
        assert_eq!(
            suspended_domains_from_l1(1_000.0).map(|domains| domains.len()),
            Some(1)
        );
        assert!(suspended_domains_from_l1(1_000.0 + SUSPENDED_DOMAINS_L1_TTL_MS).is_none());

        store_suspended_domains_l1(2_000.0, Rc::from([]));
        invalidate_suspended_domains();
        assert!(suspended_domains_from_l1(2_000.0).is_none());
    }

    #[test]
    fn url_host_suspension_covers_subdomains() {
        let suspended = ["blocked.example".to_owned()];
        assert!(url_host_within_blocked_domains(
            "https://blocked.example/inbox",
            &suspended
        ));
        assert!(url_host_within_blocked_domains(
            "https://sub.blocked.example/users/a#main-key",
            &suspended
        ));
        assert!(!url_host_within_blocked_domains(
            "https://notblocked.example/inbox",
            &suspended
        ));
        assert!(!url_host_within_blocked_domains("not a url", &suspended));
    }
}
