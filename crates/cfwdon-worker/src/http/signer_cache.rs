//! What this isolate remembers about the remote actors whose signatures it
//! checks: the actors it verified recently, so a burst of signed requests
//! from one actor reads its key from D1 once, and when it last fetched each
//! actor because a signature did not verify, so that refetch waits out
//! `cfwdon_domain::REMOTE_KEY_REFETCH_COOLDOWN_MS` even when the fetch failed
//! and left D1 untouched.

use crate::federation::RemoteActorProfile;
use std::cell::RefCell;
use std::collections::HashMap;

/// How long a verified actor's key is trusted without reading D1 again. A
/// signature this copy does not verify still reads D1, so a key another
/// isolate refreshed is seen at once.
const VERIFIED_SIGNER_TTL_MS: f64 = 60.0 * 1000.0;

/// Entries per map before it is cleared, which bounds an isolate's memory
/// against requests that name many distinct actors.
const MAX_ENTRIES: usize = 1024;

#[derive(Default)]
struct SignerCache {
    verified: HashMap<String, (RemoteActorProfile, f64)>,
    refetched: HashMap<String, f64>,
}

thread_local! {
    static SIGNER_CACHE: RefCell<SignerCache> = RefCell::new(SignerCache::default());
}

fn insert_bounded<V>(map: &mut HashMap<String, V>, key: &str, value: V) {
    if map.len() >= MAX_ENTRIES && !map.contains_key(key) {
        map.clear();
    }
    map.insert(key.to_owned(), value);
}

pub(super) fn recently_verified_signer(actor_uri: &str, now_ms: f64) -> Option<RemoteActorProfile> {
    SIGNER_CACHE.with(|cache| {
        cache
            .borrow()
            .verified
            .get(actor_uri)
            .filter(|(_, verified_at_ms)| {
                (0.0..VERIFIED_SIGNER_TTL_MS).contains(&(now_ms - verified_at_ms))
            })
            .map(|(actor, _)| actor.clone())
    })
}

pub(super) fn remember_verified_signer(actor_uri: &str, actor: &RemoteActorProfile, now_ms: f64) {
    SIGNER_CACHE.with(|cache| {
        insert_bounded(
            &mut cache.borrow_mut().verified,
            actor_uri,
            (actor.clone(), now_ms),
        );
    });
}

pub(super) fn signer_refetched_at(actor_uri: &str) -> Option<f64> {
    SIGNER_CACHE.with(|cache| cache.borrow().refetched.get(actor_uri).copied())
}

pub(super) fn remember_signer_refetch(actor_uri: &str, now_ms: f64) {
    SIGNER_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.verified.remove(actor_uri);
        insert_bounded(&mut cache.refetched, actor_uri, now_ms);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(actor_uri: &str) -> RemoteActorProfile {
        RemoteActorProfile {
            actor_uri: actor_uri.to_owned(),
            ..RemoteActorProfile::default()
        }
    }

    #[test]
    fn verified_signer_expires_and_a_refetch_forgets_it() {
        let uri = "https://remote.example/users/l1-expiry";
        remember_verified_signer(uri, &actor(uri), 1_000.0);
        assert!(recently_verified_signer(uri, 1_000.0 + VERIFIED_SIGNER_TTL_MS - 1.0).is_some());
        assert!(recently_verified_signer(uri, 1_000.0 + VERIFIED_SIGNER_TTL_MS).is_none());

        remember_verified_signer(uri, &actor(uri), 5_000.0);
        remember_signer_refetch(uri, 5_001.0);
        assert!(recently_verified_signer(uri, 5_002.0).is_none());
        assert_eq!(signer_refetched_at(uri), Some(5_001.0));
    }

    #[test]
    fn maps_stay_bounded() {
        for index in 0..=MAX_ENTRIES {
            remember_signer_refetch(&format!("https://remote.example/users/bound-{index}"), 1.0);
        }
        SIGNER_CACHE.with(|cache| assert!(cache.borrow().refetched.len() <= MAX_ENTRIES));
    }
}
