use crate::account::LocalAccount;
use crate::account::email::{AccessEmail, AccessEmailError};
use crate::account::username::{Username, UsernameError};
use crate::ids::AccountId;
use crate::quote::QuoteApprovalPolicy;
use crate::status::Visibility;
use crate::transition::Transition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationFieldIssue {
    Blank,
    InvalidFormat,
    MustBeAccepted,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RegistrationValidationErrors {
    pub username: Option<RegistrationFieldIssue>,
    pub email: Option<RegistrationFieldIssue>,
    pub password: Option<RegistrationFieldIssue>,
    pub agreement: Option<RegistrationFieldIssue>,
}

impl RegistrationValidationErrors {
    pub fn is_empty(&self) -> bool {
        self.username.is_none()
            && self.email.is_none()
            && self.password.is_none()
            && self.agreement.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationEvent {
    IntentValidated,
    AccountProvisioned,
}

/// Raw account registration input before domain validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposingRegistration {
    pub username: Option<String>,
    pub email: Option<String>,
    pub password_present: bool,
    pub agreement: Option<bool>,
}

impl ComposingRegistration {
    pub fn validate(
        self,
    ) -> Result<Transition<RegistrationIntent, RegistrationEvent>, RegistrationValidationErrors>
    {
        let mut errors = RegistrationValidationErrors::default();

        let username = match self.username.as_deref() {
            None => {
                errors.username = Some(RegistrationFieldIssue::Blank);
                None
            }
            Some(raw) => match Username::parse(raw) {
                Ok(username) => Some(username),
                Err(UsernameError::Blank) => {
                    errors.username = Some(RegistrationFieldIssue::Blank);
                    None
                }
                Err(UsernameError::InvalidCharacters) => {
                    errors.username = Some(RegistrationFieldIssue::InvalidFormat);
                    None
                }
            },
        };

        let email = match self.email.as_deref() {
            None => {
                errors.email = Some(RegistrationFieldIssue::Blank);
                None
            }
            Some(raw) => match AccessEmail::parse(raw) {
                Ok(email) => Some(email),
                Err(AccessEmailError::Blank) => {
                    errors.email = Some(RegistrationFieldIssue::Blank);
                    None
                }
                Err(AccessEmailError::Invalid) => {
                    errors.email = Some(RegistrationFieldIssue::InvalidFormat);
                    None
                }
            },
        };

        if !self.password_present {
            errors.password = Some(RegistrationFieldIssue::Blank);
        }
        if self.agreement != Some(true) {
            errors.agreement = Some(RegistrationFieldIssue::MustBeAccepted);
        }

        if errors.is_empty() {
            Ok(Transition::with_event(
                RegistrationIntent {
                    username: username.expect("validated username"),
                    email: email.expect("validated email"),
                },
                RegistrationEvent::IntentValidated,
            ))
        } else {
            Err(errors)
        }
    }
}

/// Validated registration ready for uniqueness checks and persistence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistrationIntent {
    pub username: Username,
    pub email: AccessEmail,
}

impl RegistrationIntent {
    pub fn display_name(&self) -> &str {
        self.username.as_str()
    }

    pub fn register(self, id: AccountId, keys: AccountKeyMaterial) -> RegisteringAccount {
        let display_name = self.username.as_str().to_owned();
        RegisteringAccount {
            id,
            username: self.username,
            email: self.email,
            display_name,
            keys,
        }
    }
}

/// Cryptographic identity assigned at registration time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountKeyMaterial {
    pub private_key_jwk: String,
    pub public_key_pem: String,
}

/// Account with assigned identity and keys, ready to persist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisteringAccount {
    pub id: AccountId,
    pub username: Username,
    pub email: AccessEmail,
    pub display_name: String,
    pub keys: AccountKeyMaterial,
}

impl RegisteringAccount {
    pub fn provision(self, created_at: String) -> Transition<LocalAccount, RegistrationEvent> {
        Transition::with_event(
            LocalAccount {
                id: self.id.into_inner(),
                username: self.username.into_inner(),
                access_email: self.email.into_inner(),
                display_name: self.display_name,
                bio_html: String::new(),
                bio_text: String::new(),
                fields: Vec::new(),
                locked: false,
                bot: false,
                discoverable: false,
                default_post_visibility: Visibility::Public,
                default_quote_policy: QuoteApprovalPolicy::Public,
                default_sensitive: false,
                default_language: None,
                avatar_object_key: None,
                avatar_content_type: None,
                header_object_key: None,
                header_content_type: None,
                private_key_jwk: self.keys.private_key_jwk,
                public_key_pem: self.keys.public_key_pem,
                created_at,
            },
            RegistrationEvent::AccountProvisioned,
        )
    }
}

/// OAuth/access-token driven account provisioning input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposingAccessProvision {
    pub email: String,
}

impl ComposingAccessProvision {
    pub fn resolve(
        self,
        base_username_taken: bool,
    ) -> Result<AccessProvisionIntent, AccessEmailError> {
        let email = AccessEmail::parse(&self.email)?;
        let username = Username::derive_from_email(&email, base_username_taken);
        Ok(AccessProvisionIntent { email, username })
    }
}

/// Resolved username for first-time access provisioning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessProvisionIntent {
    pub email: AccessEmail,
    pub username: Username,
}

impl AccessProvisionIntent {
    pub fn register(self, id: AccountId, keys: AccountKeyMaterial) -> RegisteringAccount {
        let display_name = self.username.as_str().to_owned();
        RegisteringAccount {
            id,
            username: self.username,
            email: self.email,
            display_name,
            keys,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_emits_intent_validated_event() {
        let transition = ComposingRegistration {
            username: Some("alice".to_owned()),
            email: Some("alice@example.com".to_owned()),
            password_present: true,
            agreement: Some(true),
        }
        .validate()
        .expect("valid registration");

        assert!(transition.has_event(&RegistrationEvent::IntentValidated));
    }

    #[test]
    fn composing_registration_requires_core_fields() {
        let errors = ComposingRegistration {
            username: None,
            email: None,
            password_present: false,
            agreement: None,
        }
        .validate()
        .unwrap_err();

        assert_eq!(errors.username, Some(RegistrationFieldIssue::Blank));
        assert_eq!(errors.email, Some(RegistrationFieldIssue::Blank));
        assert_eq!(errors.password, Some(RegistrationFieldIssue::Blank));
        assert_eq!(
            errors.agreement,
            Some(RegistrationFieldIssue::MustBeAccepted)
        );
    }

    #[test]
    fn registering_account_provisions_active_defaults() {
        let intent = ComposingRegistration {
            username: Some("alice".to_owned()),
            email: Some("alice@example.com".to_owned()),
            password_present: true,
            agreement: Some(true),
        }
        .validate()
        .expect("valid registration")
        .state;
        let registering = intent.register(
            AccountId::new("acct-1").expect("account id"),
            AccountKeyMaterial {
                private_key_jwk: "{}".to_owned(),
                public_key_pem: "pem".to_owned(),
            },
        );
        let provisioned = registering.provision("2026-01-01T00:00:00.000Z".to_owned());
        assert!(provisioned.has_event(&RegistrationEvent::AccountProvisioned));
        let account = provisioned.state;

        assert_eq!(account.username(), "alice");
        assert_eq!(account.access_email(), "alice@example.com");
        assert_eq!(account.default_quote_policy(), QuoteApprovalPolicy::Public);
        assert_eq!(account.public_key_pem(), "pem");
    }
}
