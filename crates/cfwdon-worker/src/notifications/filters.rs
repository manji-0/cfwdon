use super::{AppConfig, NotificationsQuery, remote_account_rest_id};
use cfwdon_domain::LocalAccount;

fn normalize_notification_types(values: Option<&Vec<String>>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .flat_map(|entries| entries.iter())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

pub(crate) fn notification_timestamp_sort_token(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let mut digits = value
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.len() < 14 {
        return None;
    }
    while digits.len() < 17 {
        digits.push('0');
    }
    digits.truncate(17);
    Some(digits)
}

pub(crate) fn notification_sort_key(value: &str) -> String {
    notification_timestamp_sort_token(value).unwrap_or_default()
}

pub(crate) fn notification_type_allowed(
    query: &NotificationsQuery,
    notification_type: &str,
) -> bool {
    let include = normalize_notification_types(query.types.as_ref());
    let exclude = normalize_notification_types(query.exclude_types.as_ref());
    if !include.is_empty() && !include.iter().any(|value| value == notification_type) {
        return false;
    }
    !exclude.iter().any(|value| value == notification_type)
}

pub(crate) fn notification_account_matches_filter(
    filter_account_id: Option<&str>,
    local_account_id: &str,
    remote_actor_uri: Option<&str>,
) -> bool {
    match filter_account_id {
        None => true,
        Some(filter) if filter == local_account_id => true,
        Some(filter) => remote_actor_uri
            .map(remote_account_rest_id)
            .map(|value| value == filter)
            .unwrap_or(false),
    }
}

pub(crate) fn is_admin_account(config: &AppConfig, account: &LocalAccount) -> bool {
    config
        .admin_emails
        .iter()
        .any(|email| email == &account.access_email().to_ascii_lowercase())
}

pub(crate) fn is_admin_authorized(
    config: &AppConfig,
    account: &LocalAccount,
    auth0_roles: &[String],
) -> bool {
    if is_admin_account(config, account) {
        return true;
    }
    if config.auth0_admin_roles.is_empty() {
        return false;
    }
    auth0_roles.iter().any(|role| {
        config
            .auth0_admin_roles
            .iter()
            .any(|admin_role| admin_role.eq_ignore_ascii_case(role))
    })
}
