use super::{AppConfig, instance_base_url, instance_host};
use url::Url;

pub(crate) fn local_username_from_audience_uri(config: &AppConfig, uri: &str) -> Option<String> {
    if let Some(stripped) = uri.strip_suffix("/followers") {
        return local_username_from_actor_uri(config, stripped);
    }

    local_username_from_actor_uri(config, uri)
}

pub(crate) fn local_username_from_actor_uri(config: &AppConfig, actor_uri: &str) -> Option<String> {
    let parsed = Url::parse(actor_uri).ok()?;
    let host = parsed
        .host_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host != instance_host(config) {
        return None;
    }

    let mut segments = parsed
        .path_segments()?
        .filter(|segment| !segment.is_empty());
    match (segments.next(), segments.next(), segments.next()) {
        (Some("users"), Some(username), None) => Some(username.to_ascii_lowercase()),
        _ => None,
    }
}

pub(crate) fn local_status_identity_from_uri(
    config: &AppConfig,
    uri: &str,
) -> Option<(String, String)> {
    let base = instance_base_url(config);
    let expected_prefix = format!("{base}/users/");
    let canonical = uri.trim_end_matches('/');
    if !canonical.starts_with(&expected_prefix) {
        return None;
    }
    let remainder = &canonical[expected_prefix.len()..];
    let mut segments = remainder.split('/');
    let username = segments.next()?.trim();
    let statuses = segments.next()?;
    let status_id = segments.next()?.trim();
    if statuses != "statuses"
        || username.is_empty()
        || status_id.is_empty()
        || segments.next().is_some()
    {
        return None;
    }
    Some((username.to_ascii_lowercase(), status_id.to_owned()))
}

pub(crate) fn local_username_from_status_uri(config: &AppConfig, uri: &str) -> Option<String> {
    local_status_identity_from_uri(config, uri).map(|(username, _)| username)
}
