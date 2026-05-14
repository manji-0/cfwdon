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

pub(crate) fn parse_webfinger_resource(resource: &str) -> Result<AccountHandle> {
    let resource = resource.trim();
    let Some(acct) = resource.get(5..).filter(|_| {
        resource
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("acct:"))
    }) else {
        return Err(Error::RustError(
            "WebFinger resource must use the acct: scheme".to_owned(),
        ));
    };

    let Some((username, domain)) = acct.split_once('@') else {
        return Err(Error::RustError(
            "WebFinger resource must be in acct:user@domain form".to_owned(),
        ));
    };

    let username = username.trim().to_ascii_lowercase();
    let domain = domain.trim().to_ascii_lowercase();
    if username.is_empty() || domain.is_empty() {
        return Err(Error::RustError(
            "WebFinger resource must include both username and domain".to_owned(),
        ));
    }

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

    if let Some((username, domain)) = value.split_once('@') {
        let username = username.trim().to_ascii_lowercase();
        let domain = domain.trim().to_ascii_lowercase();
        if username.is_empty() || domain.is_empty() {
            return Err(Error::RustError(
                "acct must be in user@domain form".to_owned(),
            ));
        }
        return Ok(AccountHandle {
            username,
            domain: Some(domain),
        });
    }

    Ok(AccountHandle {
        username: value.trim().to_ascii_lowercase(),
        domain: Some(instance_host(config)),
    })
}

pub(crate) fn actor_url(config: &AppConfig, username: &str) -> String {
    format!("{}/users/{}", instance_base_url(config), username)
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
