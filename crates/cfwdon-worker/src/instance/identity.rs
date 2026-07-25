use super::{AppConfig, Error, Result};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cfwdon_domain::AccountHandle;
use url::Url;

pub(crate) fn parse_csv_list(value: &str) -> Vec<String> {
    let mut values = value
        .split(',')
        .map(|entry| entry.trim().to_ascii_lowercase())
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

/// Parse a WebFinger `resource` query value.
///
/// Accepts Mastodon-compatible forms:
/// - `acct:user@domain`
/// - `user@domain`
/// - `https://domain/users/user` / `https://domain/@user`
pub(crate) fn parse_webfinger_resource(resource: &str) -> Result<AccountHandle> {
    let resource = resource.trim();
    if resource.is_empty() {
        return Err(Error::RustError(
            "WebFinger resource must not be empty".to_owned(),
        ));
    }

    if resource
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("acct:"))
    {
        return parse_acct_handle(&resource[5..]);
    }

    if resource.starts_with("http://") || resource.starts_with("https://") {
        return parse_webfinger_actor_url(resource);
    }

    if resource.contains('@') {
        return parse_acct_handle(resource.trim_start_matches('@'));
    }

    Err(Error::RustError(
        "WebFinger resource must be acct:user@domain, user@domain, or an actor URL".to_owned(),
    ))
}

fn parse_acct_handle(acct: &str) -> Result<AccountHandle> {
    // Decode only when a `%HH` escape is present. WebFinger `query_pairs()` already
    // percent-decodes once; unconditional decoding here would double-decode those values.
    let acct = maybe_percent_decode_acct(acct);
    let Some((username, domain)) = acct.split_once('@') else {
        return Err(Error::RustError(
            "WebFinger resource must be in user@domain form".to_owned(),
        ));
    };

    let username = username.trim().trim_start_matches('@').to_ascii_lowercase();
    let domain = normalize_acct_host(domain)?;
    if username.is_empty() {
        return Err(Error::RustError(
            "WebFinger resource must include both username and domain".to_owned(),
        ));
    }

    Ok(AccountHandle {
        username,
        domain: Some(domain),
    })
}

fn parse_webfinger_actor_url(resource: &str) -> Result<AccountHandle> {
    let parsed = Url::parse(resource).map_err(|_| {
        Error::RustError("WebFinger resource URL is not a valid http(s) URL".to_owned())
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(Error::RustError(
            "WebFinger resource URL must use http or https".to_owned(),
        ));
    }
    let domain = parsed
        .host_str()
        .ok_or_else(|| Error::RustError("WebFinger resource URL must include a host".to_owned()))
        .and_then(normalize_acct_host)?;

    let mut segments = parsed
        .path_segments()
        .ok_or_else(|| Error::RustError("WebFinger resource URL path is invalid".to_owned()))?
        .filter(|segment| !segment.is_empty());

    let username = match (segments.next(), segments.next(), segments.next()) {
        (Some("users"), Some(username), None) if !username.is_empty() => {
            username.to_ascii_lowercase()
        }
        (Some(segment), None, None) => segment
            .strip_prefix('@')
            .filter(|username| !username.is_empty())
            .map(|username| username.to_ascii_lowercase())
            .ok_or_else(|| {
                Error::RustError(
                    "WebFinger resource URL must point to /users/:username or /@:username"
                        .to_owned(),
                )
            })?,
        _ => {
            return Err(Error::RustError(
                "WebFinger resource URL must point to /users/:username or /@:username".to_owned(),
            ));
        }
    };

    Ok(AccountHandle {
        username,
        domain: Some(domain),
    })
}

pub(crate) fn parse_lookup_handle(value: &str, config: &AppConfig) -> Result<AccountHandle> {
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::RustError(
            "acct query parameter is required".to_owned(),
        ));
    }
    if value
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("acct:"))
    {
        return parse_webfinger_resource(value);
    }

    if value.contains('@') {
        return parse_acct_handle(value.trim_start_matches('@'));
    }

    Ok(AccountHandle {
        username: value.trim().to_ascii_lowercase(),
        domain: Some(instance_host(config)),
    })
}

fn maybe_percent_decode_acct(value: &str) -> std::borrow::Cow<'_, str> {
    if !contains_percent_escape(value) {
        return std::borrow::Cow::Borrowed(value);
    }
    match urlencoding::decode(value) {
        Ok(decoded) => std::borrow::Cow::Owned(decoded.into_owned()),
        Err(_) => std::borrow::Cow::Borrowed(value),
    }
}

fn contains_percent_escape(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes[index] == b'%'
            && bytes[index + 1].is_ascii_hexdigit()
            && bytes[index + 2].is_ascii_hexdigit()
        {
            return true;
        }
        index += 1;
    }
    false
}

/// Applies the same IDNA/punycode normalization to a configured instance domain that
/// `parse_acct_handle` applies to incoming handles, so `AccountHandle::is_local_to`
/// compares like-for-like. Scheme and path are preserved; a non-ASCII host becomes punycode.
pub(crate) fn normalize_configured_instance_domain(value: &str) -> String {
    let trimmed = value.trim();
    let (scheme, rest) = if let Some(rest) = trimmed.strip_prefix("https://") {
        ("https://", rest)
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        ("http://", rest)
    } else {
        ("", trimmed)
    };
    let (host, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };

    match normalize_acct_host(host) {
        Ok(host) => format!("{scheme}{host}{path}"),
        Err(_) => trimmed.to_owned(),
    }
}

fn normalize_acct_host(host: &str) -> Result<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return Err(Error::RustError(
            "WebFinger resource must include both username and domain".to_owned(),
        ));
    }
    let parsed = Url::parse(&format!("https://{host}")).map_err(|_| {
        Error::RustError("WebFinger resource domain is not a valid hostname".to_owned())
    })?;
    parsed
        .host_str()
        .map(|value| value.trim_end_matches('.').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::RustError("WebFinger resource domain is not a valid hostname".to_owned())
        })
}

pub(crate) fn actor_url(config: &AppConfig, username: &str) -> String {
    format!("{}/users/{}", instance_base_url(config), username)
}

/// Mastodon-style human profile URL (`/@username`).
pub(crate) fn account_profile_page_url(config: &AppConfig, username: &str) -> String {
    format!("{}/@{}", instance_base_url(config), username)
}

/// OStatus subscribe template used by Mastodon WebFinger responses.
pub(crate) fn authorize_interaction_subscribe_template(config: &AppConfig) -> String {
    format!(
        "{}/authorize_interaction?uri={{uri}}",
        instance_base_url(config)
    )
}

/// FEP-3b86 Object intent template (follow / interact with a remote object).
pub(crate) fn authorize_interaction_object_template(config: &AppConfig) -> String {
    format!(
        "{}/authorize_interaction?uri={{object}}",
        instance_base_url(config)
    )
}

/// FEP-3b86 Create intent template (compose / share).
pub(crate) fn share_create_template(config: &AppConfig) -> String {
    format!("{}/share?text={{content}}", instance_base_url(config))
}

/// FEP-2c59 acct subject for a local actor (`user@domain`).
pub(crate) fn account_webfinger_acct(config: &AppConfig, username: &str) -> String {
    format!("{username}@{}", instance_host(config))
}

pub(crate) fn webfinger_lrdd_template(config: &AppConfig) -> String {
    format!(
        "{}/.well-known/webfinger?resource={{uri}}",
        instance_base_url(config)
    )
}

pub(crate) fn normalize_instance_domain(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .split('/')
        .next()
        .unwrap_or(value.trim())
        .to_owned()
}

pub(crate) fn configured_instance_languages(config: &AppConfig) -> Vec<String> {
    if config.instance_languages.is_empty() {
        vec!["en".to_owned()]
    } else {
        config.instance_languages.clone()
    }
}

pub(crate) fn instance_supported_mime_types() -> Vec<&'static str> {
    vec![
        "image/jpeg",
        "image/png",
        "image/gif",
        "image/webp",
        "video/mp4",
        "video/webm",
        "audio/mpeg",
        "audio/mp3",
        "audio/ogg",
        "audio/webm",
    ]
}

pub(crate) fn public_key_id(config: &AppConfig, username: &str) -> String {
    format!("{}#main-key", actor_url(config, username))
}

pub(crate) fn shared_inbox_url(config: &AppConfig) -> String {
    format!("{}/inbox", instance_base_url(config))
}

pub(crate) fn remote_account_rest_id(actor_uri: &str) -> String {
    format!("r_{}", URL_SAFE_NO_PAD.encode(actor_uri.as_bytes()))
}

pub(crate) fn remote_actor_uri_from_rest_id(account_id: &str) -> Option<String> {
    let encoded = account_id.strip_prefix("r_")?;
    let bytes = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    String::from_utf8(bytes).ok()
}

pub(crate) fn instance_host(config: &AppConfig) -> String {
    config
        .instance_domain
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .split('/')
        .next()
        .unwrap_or(config.instance_domain.trim())
        .to_owned()
}

pub(crate) fn instance_base_url(config: &AppConfig) -> String {
    let domain = config.instance_domain.trim().trim_end_matches('/');
    if domain.starts_with("https://") || domain.starts_with("http://") {
        domain.to_owned()
    } else {
        format!("https://{}", instance_host(config))
    }
}

pub(crate) fn nodeinfo_url(config: &AppConfig) -> String {
    format!("{}/nodeinfo/2.0", instance_base_url(config))
}

pub(crate) fn nodeinfo_21_url(config: &AppConfig) -> String {
    format!("{}/nodeinfo/2.1", instance_base_url(config))
}

pub(crate) fn extended_description_url(config: &AppConfig) -> String {
    format!(
        "{}/api/v1/instance/extended_description",
        instance_base_url(config)
    )
}

pub(crate) fn privacy_policy_url(config: &AppConfig) -> String {
    format!(
        "{}/api/v1/instance/privacy_policy",
        instance_base_url(config)
    )
}

pub(crate) fn terms_of_service_url(config: &AppConfig) -> String {
    format!(
        "{}/api/v1/instance/terms_of_service",
        instance_base_url(config)
    )
}

pub(crate) fn peer_authority_from_uri(config: &AppConfig, uri: &str) -> Option<String> {
    let parsed = Url::parse(uri).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed
        .host_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    let default_port = match parsed.scheme() {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    };
    let authority = match (parsed.port(), default_port) {
        (Some(port), Some(scheme_default)) if port == scheme_default => host,
        (Some(port), _) => format!("{host}:{port}"),
        (None, _) => host,
    };

    if authority == instance_host(config) {
        None
    } else {
        Some(authority)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_configured_instance_domain, parse_lookup_handle, parse_webfinger_resource,
    };
    use cfwdon_core::AppConfig;

    #[test]
    fn parse_acct_decodes_percent_encoded_userpart_host() {
        let handle = parse_webfinger_resource("acct:alice%40example.com").unwrap();
        assert_eq!(handle.username, "alice");
        assert_eq!(handle.domain.as_deref(), Some("example.com"));

        let handle = parse_webfinger_resource("acct:al%69ce@example.com").unwrap();
        assert_eq!(handle.username, "alice");
        assert_eq!(handle.domain.as_deref(), Some("example.com"));
    }

    #[test]
    fn parse_acct_keeps_already_decoded_handle() {
        let handle = parse_webfinger_resource("acct:alice@example.com").unwrap();
        assert_eq!(handle.username, "alice");
        assert_eq!(handle.domain.as_deref(), Some("example.com"));
    }

    #[test]
    fn parse_acct_normalizes_unicode_domain_to_punycode() {
        let unicode = parse_webfinger_resource("acct:alice@日本.example").unwrap();
        let punycode = parse_webfinger_resource("acct:alice@xn--wgv71a.example").unwrap();
        assert_eq!(unicode.domain, punycode.domain);
        assert_eq!(unicode.domain.as_deref(), Some("xn--wgv71a.example"));
    }

    #[test]
    fn configured_instance_domain_normalization_keeps_unicode_handles_local() {
        let handle = parse_webfinger_resource("acct:alice@日本.example").unwrap();
        assert!(handle.is_local_to(&normalize_configured_instance_domain("日本.example")));
        assert!(handle.is_local_to(&normalize_configured_instance_domain(
            "https://日本.example"
        )));
        assert!(!handle.is_local_to(&normalize_configured_instance_domain("other.example")));
    }

    #[test]
    fn configured_instance_domain_normalization_preserves_ascii_input() {
        assert_eq!(
            normalize_configured_instance_domain("social.example"),
            "social.example"
        );
        assert_eq!(
            normalize_configured_instance_domain("https://social.example"),
            "https://social.example"
        );
    }

    #[test]
    fn parse_webfinger_actor_url_normalizes_unicode_host() {
        let handle = parse_webfinger_resource("https://日本.example/users/alice").unwrap();
        assert_eq!(handle.username, "alice");
        assert_eq!(handle.domain.as_deref(), Some("xn--wgv71a.example"));
    }

    #[test]
    fn parse_lookup_handle_trims_trailing_dot_like_acct() {
        let config = AppConfig::new("example.com", "cfwdon", "test instance");
        let from_lookup = parse_lookup_handle("alice@example.com.", &config).unwrap();
        let from_acct = parse_webfinger_resource("acct:alice@example.com.").unwrap();
        assert_eq!(from_lookup.username, from_acct.username);
        assert_eq!(from_lookup.domain, from_acct.domain);
        assert_eq!(from_lookup.domain.as_deref(), Some("example.com"));
    }

    #[test]
    fn parse_lookup_handle_normalizes_idn_like_acct() {
        let config = AppConfig::new("example.com", "cfwdon", "test instance");
        let from_lookup = parse_lookup_handle("alice@日本.example", &config).unwrap();
        let from_acct = parse_webfinger_resource("acct:alice@xn--wgv71a.example").unwrap();
        assert_eq!(from_lookup.domain, from_acct.domain);
        assert_eq!(from_lookup.domain.as_deref(), Some("xn--wgv71a.example"));
    }
}
