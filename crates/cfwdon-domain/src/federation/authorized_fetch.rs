//! Secure mode (Mastodon's `AUTHORIZED_FETCH`): who may read this server's own
//! ActivityPub resources. The instance actor, WebFinger, NodeInfo and
//! host-meta are not resources here: they stay public in secure mode, because
//! a remote server needs the instance actor's key to verify the signed fetches
//! secure mode asks of it.

/// A local ActivityPub resource as secure mode treats it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FetchedResource {
    /// A user's actor document. Without a usable signature it is restricted,
    /// not refused, so a remote can still read the key to verify that user.
    /// A signer the user blocks still gets it, as from Mastodon.
    Actor,
    /// What the owner posts: notes, quote stamps, the outbox, and the
    /// featured posts and tags collections. A signer the owner blocks sees
    /// none of it (Mastodon's `StatusPolicy#show?` and `for_signed_account`).
    OwnerContent,
    /// The follower and following collections. A signer the owner blocks
    /// still gets them, as from Mastodon.
    SocialGraph,
}

impl FetchedResource {
    /// Whether the owner blocking the signer changes what a signed GET gets.
    pub fn hidden_from_blocked_signer(self) -> bool {
        self == Self::OwnerContent
    }
}

/// What the `Signature` of a GET says about who asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SignedFetchVerdict {
    /// No `Signature` header, or one that does not parse.
    Unsigned,
    /// The signature omits a required header, is stale, its key cannot be
    /// fetched or does not belong to the actor, or it does not verify.
    Invalid,
    /// The key or its actor lives on a suspended domain.
    Refused,
    /// A remote actor on a domain that federates here signed the request.
    Signed,
}

/// How a GET of a local ActivityPub resource is answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FetchAccess {
    /// Secure mode is off: anyone, and shared caches may keep the answer.
    Public,
    /// Secure mode, signed by an allowed actor: the whole resource.
    Signed,
    /// Secure mode, an actor GET without a usable signature: the restricted
    /// actor document.
    Restricted,
    /// Secure mode, no usable signature: `401`.
    Unauthorized,
    /// Secure mode, the signer's domain is suspended: `403`.
    Forbidden,
}

impl FetchAccess {
    /// Whether a shared cache may keep the answer. In secure mode the answer
    /// depends on the signature, so it is `private` and varies on `Signature`.
    pub fn shared_cacheable(self) -> bool {
        self == Self::Public
    }

    /// The status a refused GET gets.
    pub fn denial_status(self) -> Option<u16> {
        match self {
            Self::Unauthorized => Some(401),
            Self::Forbidden => Some(403),
            Self::Public | Self::Signed | Self::Restricted => None,
        }
    }
}

/// What the body of an answered GET holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FetchedBody {
    /// The whole resource, as before secure mode.
    Whole,
    /// Only `id`, `type`, `preferredUsername`, `inbox`, `outbox`,
    /// `publicKey` and `endpoints` of an actor (Mastodon's
    /// `restrict_fields_to`).
    RestrictedActor,
    /// None of the owner's content: a note is `404`, a collection of the
    /// owner's posts is empty.
    Hidden,
    /// No body: the GET was refused.
    Denied,
}

/// Mastodon's secure-mode decision for one GET.
pub fn authorized_fetch_access(
    secure_mode: bool,
    resource: FetchedResource,
    verdict: SignedFetchVerdict,
) -> FetchAccess {
    if !secure_mode {
        return FetchAccess::Public;
    }
    match (verdict, resource) {
        (SignedFetchVerdict::Signed, _) => FetchAccess::Signed,
        (SignedFetchVerdict::Refused, _) => FetchAccess::Forbidden,
        (SignedFetchVerdict::Unsigned | SignedFetchVerdict::Invalid, FetchedResource::Actor) => {
            FetchAccess::Restricted
        }
        (
            SignedFetchVerdict::Unsigned | SignedFetchVerdict::Invalid,
            FetchedResource::OwnerContent | FetchedResource::SocialGraph,
        ) => FetchAccess::Unauthorized,
    }
}

/// What the answer holds, given whether the resource's owner blocks the
/// signer's account or domain. Only a signed GET names someone to block; an
/// unsigned GET in public mode cannot be told apart from anyone else.
pub fn authorized_fetch_body(
    access: FetchAccess,
    resource: FetchedResource,
    signer_blocked_by_owner: bool,
) -> FetchedBody {
    match access {
        FetchAccess::Public => FetchedBody::Whole,
        FetchAccess::Signed if signer_blocked_by_owner && resource.hidden_from_blocked_signer() => {
            FetchedBody::Hidden
        }
        FetchAccess::Signed => FetchedBody::Whole,
        FetchAccess::Restricted => FetchedBody::RestrictedActor,
        FetchAccess::Unauthorized | FetchAccess::Forbidden => FetchedBody::Denied,
    }
}

/// The actor document fields an unsigned GET sees in secure mode.
pub const RESTRICTED_ACTOR_FIELDS: &[&str] = &[
    "@context",
    "id",
    "type",
    "preferredUsername",
    "inbox",
    "outbox",
    "publicKey",
    "endpoints",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_mode_off_answers_everyone_publicly() {
        for resource in [
            FetchedResource::Actor,
            FetchedResource::OwnerContent,
            FetchedResource::SocialGraph,
        ] {
            for verdict in [
                SignedFetchVerdict::Unsigned,
                SignedFetchVerdict::Invalid,
                SignedFetchVerdict::Refused,
                SignedFetchVerdict::Signed,
            ] {
                assert_eq!(
                    authorized_fetch_access(false, resource, verdict),
                    FetchAccess::Public
                );
            }
        }
    }

    #[test]
    fn secure_mode_restricts_actors_and_refuses_objects_without_signature() {
        assert_eq!(
            authorized_fetch_access(true, FetchedResource::Actor, SignedFetchVerdict::Unsigned),
            FetchAccess::Restricted
        );
        assert_eq!(
            authorized_fetch_access(
                true,
                FetchedResource::OwnerContent,
                SignedFetchVerdict::Invalid
            ),
            FetchAccess::Unauthorized
        );
        assert_eq!(
            authorized_fetch_access(
                true,
                FetchedResource::SocialGraph,
                SignedFetchVerdict::Unsigned
            ),
            FetchAccess::Unauthorized
        );
        assert_eq!(
            authorized_fetch_access(true, FetchedResource::Actor, SignedFetchVerdict::Refused),
            FetchAccess::Forbidden
        );
        assert_eq!(FetchAccess::Unauthorized.denial_status(), Some(401));
        assert_eq!(FetchAccess::Forbidden.denial_status(), Some(403));
    }

    #[test]
    fn blocked_signer_loses_only_the_owners_content() {
        let body =
            |resource, blocked| authorized_fetch_body(FetchAccess::Signed, resource, blocked);
        assert_eq!(
            body(FetchedResource::OwnerContent, true),
            FetchedBody::Hidden
        );
        assert_eq!(
            body(FetchedResource::OwnerContent, false),
            FetchedBody::Whole
        );
        assert_eq!(body(FetchedResource::Actor, true), FetchedBody::Whole);
        assert_eq!(body(FetchedResource::SocialGraph, true), FetchedBody::Whole);
    }
}
