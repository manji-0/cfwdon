use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthProvider {
    Auth0,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthenticatedUser {
    pub provider: AuthProvider,
    pub email: String,
    pub jwt_assertion_present: bool,
    pub roles: Vec<String>,
}

impl AuthenticatedUser {
    pub fn auth0(
        email: impl Into<String>,
        jwt_assertion_present: bool,
        roles: Vec<String>,
    ) -> Self {
        Self {
            provider: AuthProvider::Auth0,
            email: email.into(),
            jwt_assertion_present,
            roles,
        }
    }
}
