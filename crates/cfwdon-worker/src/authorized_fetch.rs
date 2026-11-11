//! Secure mode (`AUTHORIZED_FETCH`, Mastodon's authorized fetch) as the
//! handlers of guarded ActivityPub routes see it: who asks, what the owner
//! hides from them, and the restricted actor document. The gate that verifies
//! the signature is `routing::authorized_fetch`.

use crate::domain_blocks::list_all_account_domain_blocks;
use crate::federation::{RemoteActorProfile, host_within_blocked_domains};
use crate::response_utils::{json_response, vary_including};
use crate::store::relationship::is_blocking_actor;
use crate::tracked_d1::D1Database;
use cfwdon_domain::{FetchAccess, FetchedBody, RESTRICTED_ACTOR_FIELDS, authorized_fetch_body};
use worker::{Request, Response, Result};

const ACTIVITYPUB_CONTENT_TYPE: &str = "application/activity+json";

/// Who a guarded handler answers.
pub(crate) enum FetchViewer {
    /// Secure mode is off, or the request is for an HTML page.
    Anyone,
    /// Secure mode, signed by this remote actor.
    Signed(RemoteActorProfile),
    /// Secure mode, an actor GET without a usable signature.
    Restricted,
}

impl FetchViewer {
    fn access(&self) -> FetchAccess {
        match self {
            Self::Anyone => FetchAccess::Public,
            Self::Signed(_) => FetchAccess::Signed,
            Self::Restricted => FetchAccess::Restricted,
        }
    }

    pub(crate) fn is_restricted(&self) -> bool {
        self.access() == FetchAccess::Restricted
    }

    pub(crate) fn is_signed(&self) -> bool {
        self.access() == FetchAccess::Signed
    }

    /// Whether `owner_account_id`'s posts are hidden from this viewer: a
    /// signer the owner blocks, by account or by domain (Mastodon's
    /// `StatusPolicy#show?` and `for_signed_account`).
    pub(crate) async fn hides_owner_content(
        &self,
        db: &D1Database,
        owner_account_id: &str,
    ) -> Result<bool> {
        let blocked = match self {
            Self::Signed(signer) => owner_blocks_signer(db, owner_account_id, signer).await?,
            Self::Anyone | Self::Restricted => false,
        };
        Ok(authorized_fetch_body(self.access(), blocked) == FetchedBody::Hidden)
    }
}

async fn owner_blocks_signer(
    db: &D1Database,
    owner_account_id: &str,
    signer: &RemoteActorProfile,
) -> Result<bool> {
    if is_blocking_actor(db, owner_account_id, &signer.actor_uri).await? {
        return Ok(true);
    }
    let blocked_domains = list_all_account_domain_blocks(db, owner_account_id).await?;
    Ok(host_within_blocked_domains(
        &signer.domain,
        &blocked_domains,
    ))
}

/// The answer depends on the `Signature`, so no shared cache may keep it:
/// one requester's answer must not reach the next.
pub(crate) fn mark_signature_dependent(mut response: Response) -> Result<Response> {
    let vary = vary_including(response.headers().get("Vary")?.as_deref(), "Signature");
    let headers = response.headers_mut();
    headers.set("Cache-Control", "private")?;
    headers.set("Vary", &vary)?;
    Ok(response)
}

/// Whether a GET of an actor or a note asks for its HTML page.
pub(crate) fn activitypub_request_prefers_html(req: &Request) -> Result<bool> {
    let accept = req.headers().get("Accept")?.unwrap_or_default();
    Ok(accept_prefers_html(&accept))
}

fn accept_prefers_html(accept: &str) -> bool {
    let accept = accept.to_ascii_lowercase();
    accept.contains("text/html")
        && !accept.contains("application/activity+json")
        && !accept.contains("application/ld+json")
}

/// What an unsigned GET of an actor sees in secure mode: what verifying its
/// signatures and delivering to it need, and nothing of the profile.
pub(crate) fn restricted_actor_document(document: &serde_json::Value) -> serde_json::Value {
    let restricted = RESTRICTED_ACTOR_FIELDS
        .iter()
        .filter_map(|field| {
            document
                .get(*field)
                .map(|value| ((*field).to_owned(), value.clone()))
        })
        .collect::<serde_json::Map<_, _>>();
    serde_json::Value::Object(restricted)
}

pub(crate) fn restricted_actor_response(document: &serde_json::Value) -> Result<Response> {
    json_response(
        &restricted_actor_document(document),
        ACTIVITYPUB_CONTENT_TYPE,
        &[("Vary", "Accept")],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restricted_actor_document_keeps_only_key_and_delivery_fields() {
        let document = serde_json::json!({
            "@context": ["https://www.w3.org/ns/activitystreams"],
            "id": "https://social.example/users/alice",
            "type": "Person",
            "preferredUsername": "alice",
            "name": "Alice",
            "summary": "<p>hello</p>",
            "inbox": "https://social.example/users/alice/inbox",
            "outbox": "https://social.example/users/alice/outbox",
            "followers": "https://social.example/users/alice/followers",
            "following": "https://social.example/users/alice/following",
            "featured": "https://social.example/users/alice/collections/featured",
            "alsoKnownAs": ["https://old.example/users/alice"],
            "publicKey": {"id": "https://social.example/users/alice#main-key"},
            "endpoints": {"sharedInbox": "https://social.example/inbox"},
        });
        let restricted = restricted_actor_document(&document);
        let mut keys = restricted
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();
        assert_eq!(
            keys,
            [
                "@context",
                "endpoints",
                "id",
                "inbox",
                "outbox",
                "preferredUsername",
                "publicKey",
                "type"
            ]
        );
        assert_eq!(restricted["publicKey"], document["publicKey"]);
    }

    #[test]
    fn html_pages_are_asked_for_by_browsers_only() {
        assert!(accept_prefers_html("text/html,application/xhtml+xml"));
        assert!(!accept_prefers_html("application/activity+json"));
        assert!(!accept_prefers_html(
            "text/html, application/ld+json; profile=\"https://www.w3.org/ns/activitystreams\""
        ));
        assert!(!accept_prefers_html(""));
    }
}
