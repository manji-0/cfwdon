use crate::account::email::AccessEmail;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UsernameError {
    #[error("username can't be blank")]
    Blank,
    #[error("username must contain only letters, numbers and underscores")]
    InvalidCharacters,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Username(String);

impl Username {
    pub fn parse(raw: &str) -> Result<Self, UsernameError> {
        let value = raw.trim().to_ascii_lowercase();
        if value.is_empty() {
            return Err(UsernameError::Blank);
        }
        if !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return Err(UsernameError::InvalidCharacters);
        }
        Ok(Self(value))
    }

    pub fn derive_from_email(email: &AccessEmail, base_username_taken: bool) -> Self {
        let sanitized = sanitize_username_local_part(email.local_part());
        if base_username_taken {
            Self(format!("{sanitized}-{}", email.short_suffix()))
        } else {
            Self(sanitized)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for Username {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for Username {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn sanitize_username_local_part(local_part: &str) -> String {
    let sanitized: String = local_part
        .chars()
        .map(|ch| ch.to_ascii_lowercase())
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect();
    if sanitized.is_empty() {
        "user".to_owned()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::email::AccessEmail;

    #[test]
    fn username_parse_rejects_invalid_characters() {
        assert_eq!(
            Username::parse("alice-bob").unwrap_err(),
            UsernameError::InvalidCharacters
        );
    }

    #[test]
    fn username_derive_from_email_appends_suffix_when_taken() {
        let email = AccessEmail::parse("alice@example.com").unwrap();
        let username = Username::derive_from_email(&email, true);
        assert!(username.as_str().starts_with("alice-"));
    }
}
