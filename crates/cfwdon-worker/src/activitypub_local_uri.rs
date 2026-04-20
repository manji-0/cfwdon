use super::{AppConfig, instance_host};
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
        (Some(segment), None, None) => segment
            .strip_prefix('@')
            .filter(|username| !username.is_empty())
            .map(|username| username.to_ascii_lowercase()),
        _ => None,
    }
}

pub(crate) fn local_status_identity_from_uri(
    config: &AppConfig,
    uri: &str,
) -> Option<(String, String)> {
    let parsed = Url::parse(uri).ok()?;
    let host = parsed
        .host_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host != instance_host(config) {
        return None;
    }

    let segments = parsed
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        ["users", username, "statuses", status_id]
            if !username.is_empty() && !status_id.is_empty() =>
        {
            Some((username.to_ascii_lowercase(), (*status_id).to_owned()))
        }
        [segment, status_id]
            if !status_id.is_empty()
                && segment
                    .strip_prefix('@')
                    .filter(|username| !username.is_empty())
                    .is_some() =>
        {
            Some((
                segment.trim_start_matches('@').to_ascii_lowercase(),
                (*status_id).to_owned(),
            ))
        }
        [segment, "statuses", status_id]
            if !status_id.is_empty()
                && segment
                    .strip_prefix('@')
                    .filter(|username| !username.is_empty())
                    .is_some() =>
        {
            Some((
                segment.trim_start_matches('@').to_ascii_lowercase(),
                (*status_id).to_owned(),
            ))
        }
        _ => None,
    }
}

pub(crate) fn local_username_from_status_uri(config: &AppConfig, uri: &str) -> Option<String> {
    local_status_identity_from_uri(config, uri).map(|(username, _)| username)
}
