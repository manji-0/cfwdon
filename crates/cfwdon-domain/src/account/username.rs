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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn username_parse_rejects_invalid_characters() {
        assert_eq!(
            Username::parse("alice-bob").unwrap_err(),
            UsernameError::InvalidCharacters
        );
    }
}
