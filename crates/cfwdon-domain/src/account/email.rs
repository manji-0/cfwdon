#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AccessEmailError {
    #[error("email can't be blank")]
    Blank,
    #[error("email is invalid")]
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AccessEmail(String);

impl AccessEmail {
    pub fn parse(raw: &str) -> Result<Self, AccessEmailError> {
        let value = raw.trim().to_ascii_lowercase();
        if value.is_empty() {
            return Err(AccessEmailError::Blank);
        }
        let Some((local, domain)) = value.split_once('@') else {
            return Err(AccessEmailError::Invalid);
        };
        if local.is_empty() || domain.is_empty() || domain.contains('@') {
            return Err(AccessEmailError::Invalid);
        }
        if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
            return Err(AccessEmailError::Invalid);
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

impl AsRef<str> for AccessEmail {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for AccessEmail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
