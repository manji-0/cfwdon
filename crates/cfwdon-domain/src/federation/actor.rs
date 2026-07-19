//! Authority binding for remote ActivityPub actor documents.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteActorAuthorityIssue {
    InvalidActorUri,
    InvalidRelatedUri,
    CrossAuthorityId,
    CrossAuthorityInbox,
    CrossAuthoritySharedInbox,
    CrossAuthorityPublicKey,
    PublicKeyOwnerMismatch,
}

/// Normalized `host` or `host:port` for an http(s) URL (default ports stripped).
pub fn remote_http_authority(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let (scheme, rest) = if let Some(rest) = trimmed.strip_prefix("https://") {
        ("https", rest)
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        ("http", rest)
    } else {
        return None;
    };

    let authority_end = rest
        .find(|ch| matches!(ch, '/' | '?' | '#'))
        .unwrap_or(rest.len());
    if authority_end == 0 {
        return None;
    }
    let authority = &rest[..authority_end];
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, host_port)| host_port)
        .unwrap_or(authority);
    if host_port.is_empty() {
        return None;
    }

    let host_port = host_port.trim_end_matches('.');
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) => {
            (host, Some(port))
        }
        _ => (host_port, None),
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }

    let default_port = match scheme {
        "http" => "80",
        "https" => "443",
        _ => return None,
    };
    match port {
        Some(port) if port == default_port => Some(host),
        Some(port) => Some(format!("{host}:{port}")),
        None => Some(host),
    }
}

pub fn remote_http_authorities_match(left: &str, right: &str) -> bool {
    match (remote_http_authority(left), remote_http_authority(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// Document `id` must share authority with the URI that was actually fetched.
pub fn remote_actor_id_authority_allowed(
    fetched_uri: &str,
    document_id: &str,
) -> Result<(), RemoteActorAuthorityIssue> {
    let fetched =
        remote_http_authority(fetched_uri).ok_or(RemoteActorAuthorityIssue::InvalidActorUri)?;
    let document =
        remote_http_authority(document_id).ok_or(RemoteActorAuthorityIssue::InvalidActorUri)?;
    if fetched == document {
        Ok(())
    } else {
        Err(RemoteActorAuthorityIssue::CrossAuthorityId)
    }
}

pub fn remote_actor_related_uri_authority_allowed(
    actor_uri: &str,
    related_uri: &str,
    issue: RemoteActorAuthorityIssue,
) -> Result<(), RemoteActorAuthorityIssue> {
    let actor =
        remote_http_authority(actor_uri).ok_or(RemoteActorAuthorityIssue::InvalidActorUri)?;
    let related =
        remote_http_authority(related_uri).ok_or(RemoteActorAuthorityIssue::InvalidRelatedUri)?;
    if actor == related { Ok(()) } else { Err(issue) }
}

/// `publicKey.owner` may be absent; when present it must match the actor URI.
pub fn remote_actor_public_key_owner_allowed(
    actor_uri: &str,
    owner: Option<&str>,
) -> Result<(), RemoteActorAuthorityIssue> {
    let Some(owner) = owner.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if owner == actor_uri.trim() {
        return Ok(());
    }
    // Some servers omit fragments on owner while key id keeps them; compare without fragment.
    let actor_base = actor_uri
        .trim()
        .split('#')
        .next()
        .unwrap_or(actor_uri)
        .trim();
    let owner_base = owner.split('#').next().unwrap_or(owner).trim();
    if owner_base == actor_base {
        Ok(())
    } else {
        Err(RemoteActorAuthorityIssue::PublicKeyOwnerMismatch)
    }
}

pub fn webfinger_link_is_activitypub_type(link_type: Option<&str>) -> bool {
    let Some(link_type) = link_type.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let lower = link_type.to_ascii_lowercase();
    lower.starts_with("application/activity+json") || lower.starts_with("application/ld+json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_strips_default_ports_and_paths() {
        assert_eq!(
            remote_http_authority("https://Remote.Example/users/alice"),
            Some("remote.example".to_owned())
        );
        assert_eq!(
            remote_http_authority("https://remote.example:443/users/alice#key"),
            Some("remote.example".to_owned())
        );
        assert_eq!(
            remote_http_authority("https://remote.example:8443/users/alice"),
            Some("remote.example:8443".to_owned())
        );
    }

    #[test]
    fn id_authority_rejects_cross_host_documents() {
        assert!(
            remote_actor_id_authority_allowed(
                "https://evil.example/fake",
                "https://victim.social/users/alice",
            )
            .is_err()
        );
        assert!(
            remote_actor_id_authority_allowed(
                "https://remote.example/users/alice",
                "https://remote.example/@alice",
            )
            .is_ok()
        );
    }

    #[test]
    fn public_key_owner_allows_absent_or_matching() {
        assert!(
            remote_actor_public_key_owner_allowed("https://remote.example/users/alice", None)
                .is_ok()
        );
        assert!(
            remote_actor_public_key_owner_allowed(
                "https://remote.example/users/alice",
                Some("https://remote.example/users/alice")
            )
            .is_ok()
        );
        assert!(
            remote_actor_public_key_owner_allowed(
                "https://remote.example/users/alice",
                Some("https://evil.example/users/alice")
            )
            .is_err()
        );
    }

    #[test]
    fn webfinger_activitypub_type_detection() {
        assert!(webfinger_link_is_activitypub_type(Some(
            "application/activity+json"
        )));
        assert!(webfinger_link_is_activitypub_type(Some(
            "application/ld+json; profile=\"https://www.w3.org/ns/activitystreams\""
        )));
        assert!(!webfinger_link_is_activitypub_type(Some("text/html")));
        assert!(!webfinger_link_is_activitypub_type(None));
    }
}
