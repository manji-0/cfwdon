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
        if !value.contains('@') {
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

    pub fn local_part(&self) -> &str {
        self.0.split('@').next().unwrap_or("user")
    }

    pub fn short_suffix(&self) -> String {
        let checksum = self.0.bytes().fold(0u32, |acc, byte| {
            acc.wrapping_mul(16777619).wrapping_add(byte as u32)
        });
        format!("{:06x}", checksum & 0x00ff_ffff)
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
