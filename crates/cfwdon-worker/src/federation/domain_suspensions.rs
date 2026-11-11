//! Admin instance domain blocks (`instance_domain_blocks`) as enforcement reads
//! them. A `suspend` block refuses inbound activities, outbound deliveries, and
//! remote fetches for the domain and its subdomains; `silence`, `reject_media`,
//! and `reject_reports` are read through [`InstanceDomainBlocks`].

use crate::db_utils::d1_results;
use crate::tracked_d1::D1Database;
use cfwdon_domain::{
    DomainBlockPolicy, DomainBlockSeverity, DomainRestrictions, domain_restrictions_for_host,
    host_within_any_blocked_domain,
};
use serde::Deserialize;
use std::cell::RefCell;
use std::rc::Rc;
use url::Url;
use worker::{Env, Error, Result};

/// Other isolates pick up admin block edits within this window.
const DOMAIN_BLOCKS_L1_TTL_MS: f64 = 60_000.0;

thread_local! {
    static DOMAIN_BLOCKS_DB: RefCell<Option<D1Database>> = const { RefCell::new(None) };
    /// `(loaded_at_ms, blocks)`; every federation request and fetch reads it.
    static DOMAIN_BLOCKS_L1: RefCell<Option<(f64, Rc<InstanceDomainBlocks>)>> =
        const { RefCell::new(None) };
}

/// Install the D1 binding the block set loads from. Missing bindings leave
/// enforcement off, so unit paths without D1 keep working.
pub(crate) fn install_domain_suspensions(env: &Env, database_binding: &str) {
    let db = env.d1(database_binding).ok().map(D1Database::new);
    DOMAIN_BLOCKS_DB.with(|slot| *slot.borrow_mut() = db);
}

/// One instance domain block, without its admin-only fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CachedDomainBlock {
    pub(crate) domain: String,
    pub(crate) policy: DomainBlockPolicy,
    pub(crate) obfuscate: bool,
    pub(crate) public_comment: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct InstanceDomainBlocks {
    blocks: Vec<CachedDomainBlock>,
    suspended: Rc<[String]>,
    /// Some block has `severity: silence`.
    has_silence_block: bool,
}

impl InstanceDomainBlocks {
    pub(crate) fn new(blocks: Vec<CachedDomainBlock>) -> Self {
        let suspended = blocks
            .iter()
            .filter(|block| block.policy.severity == DomainBlockSeverity::Suspend)
            .map(|block| block.domain.clone())
            .collect();
        let has_silence_block = blocks
            .iter()
            .any(|block| block.policy.severity == DomainBlockSeverity::Silence);
        Self {
            blocks,
            suspended,
            has_silence_block,
        }
    }

    pub(crate) fn blocks(&self) -> &[CachedDomainBlock] {
        &self.blocks
    }

    /// Whether any block silences, so a silence check can skip loading its
    /// authors' acct domains (and their follows) when none does. A `suspend`
    /// block also silences, and like a `silence` block it can cover an author
    /// through an acct domain only the stored actor row knows, so either kind
    /// counts. Every author [`Self::restrictions_for_actor`] reports silenced
    /// passes this check.
    pub(crate) fn may_silence(&self) -> bool {
        self.has_silence_block || !self.suspended.is_empty()
    }

    pub(crate) fn restrictions_for_host(&self, host: &str) -> DomainRestrictions {
        domain_restrictions_for_host(
            host,
            self.blocks
                .iter()
                .map(|block| (block.domain.as_str(), block.policy)),
        )
    }

    /// Restrictions for a remote account, by its acct domain and by the host
    /// of its actor URI (they differ under WebFinger delegation).
    pub(crate) fn restrictions_for_actor(
        &self,
        domain: &str,
        actor_uri: &str,
    ) -> DomainRestrictions {
        let by_domain = self.restrictions_for_host(domain);
        let by_uri = self.restrictions_for_url(actor_uri);
        DomainRestrictions {
            suspended: by_domain.suspended || by_uri.suspended,
            silenced: by_domain.silenced || by_uri.silenced,
            reject_media: by_domain.reject_media || by_uri.reject_media,
            reject_reports: by_domain.reject_reports || by_uri.reject_reports,
        }
    }

    /// Restrictions for the host of `url`; an unparseable URL has none.
    pub(crate) fn restrictions_for_url(&self, url: &str) -> DomainRestrictions {
        Url::parse(url)
            .ok()
            .and_then(|url| url.host_str().map(|host| self.restrictions_for_host(host)))
            .unwrap_or_default()
    }
}

fn domain_blocks_from_l1(now_ms: f64) -> Option<Rc<InstanceDomainBlocks>> {
    DOMAIN_BLOCKS_L1.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|(loaded_at_ms, _)| now_ms - loaded_at_ms < DOMAIN_BLOCKS_L1_TTL_MS)
            .map(|(_, blocks)| Rc::clone(blocks))
    })
}

fn store_domain_blocks_l1(now_ms: f64, blocks: Rc<InstanceDomainBlocks>) {
    DOMAIN_BLOCKS_L1.with(|slot| *slot.borrow_mut() = Some((now_ms, blocks)));
}

/// Drop this isolate's cached set after an admin adds, changes, or removes a block.
pub(crate) fn invalidate_suspended_domains() {
    DOMAIN_BLOCKS_L1.with(|slot| *slot.borrow_mut() = None);
}

#[derive(Deserialize)]
struct DomainBlockRow {
    domain: String,
    severity: String,
    reject_media: i64,
    reject_reports: i64,
    obfuscate: i64,
    public_comment: Option<String>,
}

impl From<DomainBlockRow> for CachedDomainBlock {
    fn from(row: DomainBlockRow) -> Self {
        Self {
            domain: row.domain,
            policy: DomainBlockPolicy {
                // The column's CHECK constraint names every severity; an
                // unknown value fails closed.
                severity: DomainBlockSeverity::from_name(&row.severity)
                    .unwrap_or(DomainBlockSeverity::Suspend),
                reject_media: row.reject_media != 0,
                reject_reports: row.reject_reports != 0,
            },
            obfuscate: row.obfuscate != 0,
            public_comment: row.public_comment.filter(|comment| !comment.is_empty()),
        }
    }
}

/// Every instance domain block, from this isolate's cache or D1.
pub(crate) async fn instance_domain_blocks() -> Result<Rc<InstanceDomainBlocks>> {
    let Some(db) = DOMAIN_BLOCKS_DB.with(|slot| slot.borrow().as_ref().map(D1Database::detached))
    else {
        return Ok(Rc::default());
    };
    let now_ms = js_sys::Date::now();
    if let Some(blocks) = domain_blocks_from_l1(now_ms) {
        return Ok(blocks);
    }
    let result = db
        .prepare(
            "SELECT domain, severity, reject_media, reject_reports, obfuscate, public_comment
             FROM instance_domain_blocks
             ORDER BY domain ASC",
        )
        .all()
        .await?;
    let blocks = Rc::new(InstanceDomainBlocks::new(
        d1_results::<DomainBlockRow>(&result)?
            .into_iter()
            .map(CachedDomainBlock::from)
            .collect(),
    ));
    store_domain_blocks_l1(now_ms, Rc::clone(&blocks));
    Ok(blocks)
}

/// Every suspended domain, from this isolate's cache or D1.
pub(crate) async fn suspended_instance_domains() -> Result<Rc<[String]>> {
    Ok(Rc::clone(&instance_domain_blocks().await?.suspended))
}

/// What the instance refuses for a remote account.
pub(crate) async fn remote_actor_domain_restrictions(
    domain: &str,
    actor_uri: &str,
) -> Result<DomainRestrictions> {
    Ok(instance_domain_blocks()
        .await?
        .restrictions_for_actor(domain, actor_uri))
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
        CachedDomainBlock, DOMAIN_BLOCKS_L1_TTL_MS, InstanceDomainBlocks, domain_blocks_from_l1,
        invalidate_suspended_domains, store_domain_blocks_l1, url_host_within_blocked_domains,
    };
    use cfwdon_domain::{DomainBlockPolicy, DomainBlockSeverity};
    use std::rc::Rc;

    fn block(domain: &str, severity: DomainBlockSeverity) -> CachedDomainBlock {
        CachedDomainBlock {
            domain: domain.to_owned(),
            policy: DomainBlockPolicy {
                severity,
                reject_media: false,
                reject_reports: false,
            },
            obfuscate: false,
            public_comment: None,
        }
    }

    #[test]
    fn domain_blocks_l1_expires_and_invalidates() {
        store_domain_blocks_l1(
            1_000.0,
            Rc::new(InstanceDomainBlocks::new(vec![block(
                "blocked.example",
                DomainBlockSeverity::Suspend,
            )])),
        );
        assert_eq!(
            domain_blocks_from_l1(1_000.0).map(|blocks| blocks.blocks().len()),
            Some(1)
        );
        assert!(domain_blocks_from_l1(1_000.0 + DOMAIN_BLOCKS_L1_TTL_MS).is_none());

        store_domain_blocks_l1(2_000.0, Rc::default());
        invalidate_suspended_domains();
        assert!(domain_blocks_from_l1(2_000.0).is_none());
    }

    #[test]
    fn only_suspend_blocks_refuse_federation() {
        let blocks = InstanceDomainBlocks::new(vec![
            block("suspended.example", DomainBlockSeverity::Suspend),
            block("silenced.example", DomainBlockSeverity::Silence),
            block("noop.example", DomainBlockSeverity::Noop),
        ]);
        assert_eq!(&*blocks.suspended, ["suspended.example".to_owned()]);
        let silenced = blocks.restrictions_for_url("https://a.silenced.example/users/bob");
        assert!(silenced.silenced && !silenced.suspended);
        let delegated =
            blocks.restrictions_for_actor("silenced.example", "https://ap.other.example/users/bob");
        assert!(delegated.silenced);
        assert!(blocks.restrictions_for_url("not a url") == Default::default());
    }

    #[test]
    fn silence_check_is_skipped_only_when_no_author_can_be_silenced() {
        let delegated = ("blocked.example", "https://ap.other.example/users/alice");
        let by_host = ("other.example", "https://a.blocked.example/users/bob");
        let unblocked = ("remote.example", "https://remote.example/users/carol");
        assert!(!InstanceDomainBlocks::default().may_silence());
        assert!(
            !InstanceDomainBlocks::new(vec![block("blocked.example", DomainBlockSeverity::Noop)])
                .may_silence()
        );
        for severity in [DomainBlockSeverity::Silence, DomainBlockSeverity::Suspend] {
            let blocks = InstanceDomainBlocks::new(vec![block("blocked.example", severity)]);
            assert!(blocks.may_silence(), "{severity:?}");
            for (domain, actor_uri) in [delegated, by_host] {
                assert!(
                    blocks.restrictions_for_actor(domain, actor_uri).silenced,
                    "{severity:?} silences {domain} {actor_uri}"
                );
            }
            assert!(
                !blocks
                    .restrictions_for_actor(unblocked.0, unblocked.1)
                    .silenced
            );
        }
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
