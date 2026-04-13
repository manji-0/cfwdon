use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthProvider {
    CloudflareAccess,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthenticatedUser {
    pub provider: AuthProvider,
    pub email: String,
    pub jwt_assertion_present: bool,
}

impl AuthenticatedUser {
    pub fn cloudflare_access(email: impl Into<String>, jwt_assertion_present: bool) -> Self {
        Self {
            provider: AuthProvider::CloudflareAccess,
            email: email.into(),
            jwt_assertion_present,
        }
    }
}
