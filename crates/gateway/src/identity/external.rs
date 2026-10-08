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

/// Who the provider says the user is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalIdentity {
    /// The provider's id, as `SignInProvider::id`; stored as `auth_provider`.
    pub provider: &'static str,
    /// Stable and unique across providers of one kind: `<issuer>|<subject>`.
    pub external_id: String,
    pub email: String,
    /// Whether the provider vouches that the user controls `email`.
    pub email_verified: bool,
    pub name: Option<String>,
    pub groups: Vec<String>,
}

/// Where to send the browser, and what to remember until it returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Begin {
    pub redirect_to: String,
    /// The value of the flow cookie. It is protected by the provider; the
    /// caller only stores it and hands it back to `complete`.
    pub flow_cookie: String,
}

/// The query of the browser's return from the provider.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    /// Set by the provider when the user or the provider refused.
    pub error: Option<String>,
    pub error_description: Option<String>,
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

    /// Finishes a sign-in from the browser's return and the flow cookie
    /// value that `begin` made.
    fn complete<'a>(
        &'a self,
        query: &'a CallbackQuery,
        flow_cookie: &'a str,
    ) -> BoxFuture<'a, Result<ExternalIdentity, ExternalError>>;
}

/// Makes the OpenID Connect provider from complete settings. The flow
/// itself is not part of this change: until it is, no provider is made.
pub fn build_oidc(
    _settings: &crate::store::OidcSettings,
    _client_secret: &str,
    _redirect_uri: &str,
    _http: &reqwest::Client,
) -> Option<std::sync::Arc<dyn SignInProvider>> {
    None
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

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
            query: &'a CallbackQuery,
            flow_cookie: &'a str,
        ) -> BoxFuture<'a, Result<ExternalIdentity, ExternalError>> {
            Box::pin(async move {
                if query.state.as_deref() != Some(flow_cookie) {
                    return Err(ExternalError::BadState);
                }
                Ok(ExternalIdentity {
                    provider: "fixed",
                    external_id: "issuer|sub".into(),
                    email: "a@example.com".into(),
                    email_verified: true,
                    name: None,
                    groups: vec![],
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
        let query = CallbackQuery {
            state: Some(begun.flow_cookie.clone()),
            ..CallbackQuery::default()
        };
        let who = provider.complete(&query, &begun.flow_cookie).await.unwrap();
        assert_eq!(who.external_id, "issuer|sub");
        let wrong = CallbackQuery::default();
        assert_eq!(
            provider.complete(&wrong, &begun.flow_cookie).await,
            Err(ExternalError::BadState)
        );
    }
}
