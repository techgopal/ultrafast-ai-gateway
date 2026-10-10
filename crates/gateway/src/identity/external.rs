//! Sign-in with an identity provider outside the gateway.
//!
//! A [`SignInProvider`] runs the two halves of a browser redirect flow:
//! [`begin`](SignInProvider::begin) makes the address to send the browser to
//! and a cookie value that carries the flow's state, and
//! [`complete`](SignInProvider::complete) turns the browser's return, with
//! that cookie value, into the identity the provider vouches for. It decides
//! nothing about users: the gateway maps an [`ExternalIdentity`] to one of its
//! own users and makes the session, so every provider ends in the same
//! session a password sign-in makes.
//!
//! The trait is object safe so `AppState` can hold the current provider as an
//! `Arc<dyn SignInProvider>` and swap it when the settings change. Its methods
//! return boxed futures rather than using `async fn`, which would make it
//! unusable as a trait object.

use std::future::Future;
use std::pin::Pin;

use thiserror::Error;

/// The boxed future a provider method returns.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Who the provider says the user is. `Debug` shows no personal data: an
/// identity is never printed, whatever the caller does.
#[derive(Clone, PartialEq, Eq)]
pub struct ExternalIdentity {
    /// The provider's id, as `SignInProvider::id`; stored as `auth_provider`.
    pub provider: &'static str,
    /// Stable and unique across providers of one kind: `<issuer>|<subject>`.
    pub external_id: String,
    pub email: String,
    /// Whether the provider vouches that the user controls `email`.
    pub email_verified: bool,
    pub name: Option<String>,
    /// The groups the provider listed. `None`: it said nothing about groups
    /// (no such claim, or one too large to include), which is not the same
    /// as `Some(vec![])`, a user who is in none. Roles are left alone when
    /// the groups are unknown.
    pub groups: Option<Vec<String>>,
}

impl std::fmt::Debug for ExternalIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalIdentity")
            .field("provider", &self.provider)
            .field("email_verified", &self.email_verified)
            .finish_non_exhaustive()
    }
}

/// A finished sign-in: who the provider vouches for, and where to send the
/// browser next. The provider owns its relay state (OIDC keeps the path in
/// its flow cookie, SAML in RelayState), so the caller never opens it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completed {
    pub identity: ExternalIdentity,
    /// The path `begin` was given; the caller checks it again before use.
    pub return_to: String,
}

/// Where to send the browser, and what to remember until it returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Begin {
    pub redirect_to: String,
    /// The value of the flow cookie. It is protected by the provider; the
    /// caller only stores it and hands it back to `complete`.
    pub flow_cookie: String,
}

/// What the browser brought back from the provider, as name/value pairs.
/// The transport does not matter to a provider: the pairs come from the
/// query string of a redirect (OpenID Connect) or from a form POST body
/// (SAML and OIDC's `form_post`), and the caller collects them either way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallbackParams(Vec<(String, String)>);

impl CallbackParams {
    pub fn new(pairs: Vec<(String, String)>) -> Self {
        Self(pairs)
    }

    /// The value of `name`. A name that was sent more than once is
    /// ambiguous and counts as absent.
    pub fn get(&self, name: &str) -> Option<&str> {
        let mut found = self.0.iter().filter(|(k, _)| k == name);
        match (found.next(), found.next()) {
            (Some((_, v)), None) => Some(v.as_str()),
            _ => None,
        }
    }
}

impl FromIterator<(String, String)> for CallbackParams {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(pairs: I) -> Self {
        Self(pairs.into_iter().collect())
    }
}

impl<'a> FromIterator<(&'a str, &'a str)> for CallbackParams {
    fn from_iter<I: IntoIterator<Item = (&'a str, &'a str)>>(pairs: I) -> Self {
        Self(
            pairs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }
}

/// Why a sign-in could not be completed. No message holds a token, a secret
/// or the text of a response.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExternalError {
    #[error("single sign-on is not set up")]
    NotConfigured,
    #[error("the provider's discovery document could not be used: {0}")]
    Discovery(String),
    /// The `state` does not match, or the flow cookie is missing or altered.
    #[error("the sign-in state does not match")]
    BadState,
    #[error("the sign-in took too long")]
    Expired,
    #[error("the provider refused the sign-in: {0}")]
    IdpError(String),
    #[error("the ID token was not accepted: {0}")]
    Token(String),
    #[error("the code could not be exchanged: {0}")]
    Exchange(String),
}

pub trait SignInProvider: Send + Sync {
    /// Stable, lower case: `oidc`. Stored as the user's `auth_provider`.
    fn id(&self) -> &'static str;

    /// The name of the provider on the sign-in button.
    fn label(&self) -> &str;

    /// Starts a sign-in. `return_to` is a path inside the console that the
    /// caller has already checked; the provider keeps it in the flow cookie.
    fn begin<'a>(&'a self, return_to: &'a str) -> BoxFuture<'a, Result<Begin, ExternalError>>;

    /// Finishes a sign-in from the parameters of the browser's return and the flow cookie
    /// value that `begin` made.
    fn complete<'a>(
        &'a self,
        params: &'a CallbackParams,
        flow_cookie: &'a str,
    ) -> BoxFuture<'a, Result<Completed, ExternalError>>;
}

/// Makes the OpenID Connect provider from complete settings. `cipher`
/// protects the flow cookie. Nothing is fetched here: the provider reads
/// its discovery document when the first sign-in starts.
pub fn build_oidc(
    settings: &crate::store::OidcSettings,
    client_secret: &str,
    redirect_uri: &str,
    http: &reqwest::Client,
    cipher: &crate::secrets::Cipher,
) -> Option<std::sync::Arc<dyn SignInProvider>> {
    Some(std::sync::Arc::new(super::oidc::OidcProvider::new(
        settings,
        client_secret,
        redirect_uri,
        http,
        cipher,
    )))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn params_read_a_name_once_and_refuse_an_ambiguous_one() {
        let params: CallbackParams = [("a", "1"), ("b", "2"), ("b", "3")].into_iter().collect();
        assert_eq!(params.get("a"), Some("1"));
        assert_eq!(params.get("b"), None);
        assert_eq!(params.get("c"), None);
        let owned = CallbackParams::new(vec![("x".into(), "y".into())]);
        assert_eq!(owned.get("x"), Some("y"));
    }

    /// A provider that needs nothing outside the process.
    struct Fixed;

    impl SignInProvider for Fixed {
        fn id(&self) -> &'static str {
            "fixed"
        }
        fn label(&self) -> &str {
            "Fixed"
        }
        fn begin<'a>(&'a self, return_to: &'a str) -> BoxFuture<'a, Result<Begin, ExternalError>> {
            Box::pin(async move {
                Ok(Begin {
                    redirect_to: "https://idp.example.com/authorize".into(),
                    flow_cookie: return_to.to_string(),
                })
            })
        }
        fn complete<'a>(
            &'a self,
            params: &'a CallbackParams,
            flow_cookie: &'a str,
        ) -> BoxFuture<'a, Result<Completed, ExternalError>> {
            Box::pin(async move {
                if params.get("state") != Some(flow_cookie) {
                    return Err(ExternalError::BadState);
                }
                Ok(Completed {
                    identity: ExternalIdentity {
                        provider: "fixed",
                        external_id: "issuer|sub".into(),
                        email: "a@example.com".into(),
                        email_verified: true,
                        name: None,
                        groups: None,
                    },
                    return_to: "/keys".into(),
                })
            })
        }
    }

    #[tokio::test]
    async fn a_provider_works_as_a_shared_trait_object() {
        let provider: Arc<dyn SignInProvider> = Arc::new(Fixed);
        let shared = provider.clone();
        let begun = tokio::spawn(async move { shared.begin("/keys").await })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(provider.id(), "fixed");
        assert_eq!(provider.label(), "Fixed");
        let params: CallbackParams = [("state", begun.flow_cookie.as_str())]
            .into_iter()
            .collect();
        let who = provider
            .complete(&params, &begun.flow_cookie)
            .await
            .unwrap();
        assert_eq!(who.identity.external_id, "issuer|sub");
        assert_eq!(who.return_to, "/keys");
        let wrong = CallbackParams::default();
        assert_eq!(
            provider.complete(&wrong, &begun.flow_cookie).await,
            Err(ExternalError::BadState)
        );
    }

    #[test]
    fn an_identity_is_never_printed() {
        let who = ExternalIdentity {
            provider: "oidc",
            external_id: "https://idp.example.com|sub-secret".into(),
            email: "ann@example.com".into(),
            email_verified: true,
            name: Some("Ann Example".into()),
            groups: Some(vec!["admins".into()]),
        };
        let shown = format!(
            "{who:?} {:?}",
            Completed {
                identity: who.clone(),
                return_to: "/".into()
            }
        );
        for private in ["ann@", "Ann", "sub-secret", "admins", "idp.example.com"] {
            assert!(!shown.contains(private), "{private} in {shown}");
        }
    }
}
