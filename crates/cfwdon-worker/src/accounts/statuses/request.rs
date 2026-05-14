use crate::{Error, Result};

pub(crate) fn required_account_status_route_param(
    value: Option<&str>,
    name: &str,
) -> Result<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError(format!("missing {name} route parameter")))
}

pub(crate) fn required_account_status_username_param(value: Option<&str>) -> Result<String> {
    required_account_status_route_param(value, "username").map(|value| value.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_account_status_route_param_trims_values() {
        assert_eq!(
            required_account_status_route_param(Some("  account-1  "), "id").unwrap(),
            "account-1"
        );
    }

    #[test]
    fn required_account_status_route_param_rejects_missing_or_blank_values() {
        assert!(required_account_status_route_param(None, "id").is_err());
        assert!(required_account_status_route_param(Some("  "), "id").is_err());
    }

    #[test]
    fn required_account_status_username_param_normalizes_case() {
        assert_eq!(
            required_account_status_username_param(Some("  Alice  ")).unwrap(),
            "alice"
        );
    }
}
