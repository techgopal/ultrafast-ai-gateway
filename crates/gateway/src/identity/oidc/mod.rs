//! Sign-in with an OpenID Connect provider (authorization code flow with
//! PKCE). [`OidcProvider`] is the [`SignInProvider`] the gateway holds while
//! single sign-on is on.

pub mod discovery;
pub mod flow;
pub mod token;

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Url;
use serde_json::Value;

use self::discovery::{Discovery, DiscoveryCache, Jwks};
use self::flow::FlowState;
use self::token::{Claims, Expected};
use super::external::{
    Begin, BoxFuture, CallbackParams, Completed, ExternalError, ExternalIdentity, SignInProvider,
};
use crate::secrets::Cipher;

/// Entra ID issues tokens for every tenant under this prefix.
const ENTRA_ISSUER_PREFIX: &str = "https://login.microsoftonline.com/";

pub struct OidcProvider {
    label: String,
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    scope: String,
    groups_claim: String,
    http: reqwest::Client,
    cipher: Cipher,
    issuer: String,
    discovery: DiscoveryCache,
    jwks: Mutex<Option<Arc<Jwks>>>,
}

/// The scopes always sent.
const BASE_SCOPE: [&str; 3] = ["openid", "email", "profile"];
/// The longest error code taken from a callback.
const MAX_ERROR_CODE: usize = 64;

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// `application/x-www-form-urlencoded` text of one value (RFC 6749, appendix B).
fn form_encode(value: &str) -> String {
    let mut url = Url::parse("http://form.invalid/").expect("a constant URL parses");
    url.query_pairs_mut().append_pair("v", value);
    url.query()
        .unwrap_or("v=")
        .trim_start_matches("v=")
        .to_string()
}

/// The error code of a callback, reduced to letters, digits and `_`.
fn clean_error_code(raw: &str) -> String {
    let code: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(MAX_ERROR_CODE)
        .collect();
    if code.is_empty() {
        "error".to_string()
    } else {
        code
    }
}

impl OidcProvider {
    pub fn new(
        settings: &crate::store::OidcSettings,
        client_secret: &str,
        redirect_uri: &str,
        http: &reqwest::Client,
        cipher: &Cipher,
    ) -> Self {
        let mut scopes: Vec<&str> = BASE_SCOPE.to_vec();
        for extra in settings.scopes.split_whitespace() {
            if !scopes.contains(&extra) {
                scopes.push(extra);
            }
        }
        Self {
            label: settings.label.clone(),
            client_id: settings.client_id.clone(),
            client_secret: client_secret.to_string(),
            redirect_uri: redirect_uri.to_string(),
            scope: scopes.join(" "),
            groups_claim: settings.groups_claim.clone(),
            http: http.clone(),
            cipher: cipher.clone(),
            issuer: settings.issuer.clone(),
            discovery: DiscoveryCache::new(http.clone(), &settings.issuer),
            jwks: Mutex::new(None),
        }
    }

    fn jwks_for(&self, discovery: &Discovery) -> Arc<Jwks> {
        let mut held = self.jwks.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(jwks) = held.as_ref() {
            if jwks.uri() == &discovery.jwks_uri {
                return jwks.clone();
            }
        }
        let jwks = Arc::new(Jwks::new(self.http.clone(), discovery.jwks_uri.clone()));
        *held = Some(jwks.clone());
        jwks
    }

    /// Trades the code for the tokens. Returns the ID token and the access
    /// token; the refresh token is never read.
    async fn exchange(
        &self,
        discovery: &Discovery,
        code: &str,
        verifier: &str,
    ) -> Result<(String, Option<String>), ExternalError> {
        let mut form = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code.to_string()),
            ("redirect_uri", self.redirect_uri.clone()),
            ("code_verifier", verifier.to_string()),
        ];
        let mut request = self
            .http
            .post(discovery.token_endpoint.clone())
            .header("accept", "application/json")
            .timeout(discovery::REQUEST_TIMEOUT);
        if discovery.basic_auth {
            request = request.basic_auth(
                form_encode(&self.client_id),
                Some(form_encode(&self.client_secret)),
            );
        } else {
            form.push(("client_id", self.client_id.clone()));
            form.push(("client_secret", self.client_secret.clone()));
        }
        let mut body = Url::parse("http://form.invalid/").expect("a constant URL parses");
        body.query_pairs_mut()
            .extend_pairs(form.iter().map(|(k, v)| (*k, v.as_str())));
        let request = request
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body.query().unwrap_or_default().to_string());
        let answer = discovery::read_json(request.send().await)
            .await
            .map_err(ExternalError::Exchange)?;
        let text = |name: &str| {
            answer
                .get(name)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        let id_token = text("id_token")
            .ok_or_else(|| ExternalError::Token("the answer holds no ID token".to_string()))?;
        Ok((id_token, text("access_token")))
    }

    /// The claims the userinfo endpoint holds for the access token, only if
    /// they are about the same subject.
    async fn user_info(
        &self,
        endpoint: &Url,
        access_token: &str,
        subject: &str,
    ) -> Result<Claims, ExternalError> {
        let answer = discovery::get_json(&self.http, endpoint, Some(access_token))
            .await
            .map_err(|_| ExternalError::Token("the userinfo answer was not usable".to_string()))?;
        let Value::Object(raw) = answer else {
            return Err(ExternalError::Token(
                "the userinfo answer was not usable".to_string(),
            ));
        };
        if raw.get("sub").and_then(Value::as_str) != Some(subject) {
            return Err(ExternalError::Token(
                "the userinfo subject differs".to_string(),
            ));
        }
        Ok(Claims {
            subject: subject.to_string(),
            raw,
        })
    }
}

impl SignInProvider for OidcProvider {
    fn id(&self) -> &'static str {
        "oidc"
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn begin<'a>(&'a self, return_to: &'a str) -> BoxFuture<'a, Result<Begin, ExternalError>> {
        Box::pin(async move {
            let discovery = self.discovery.get().await?;
            let flow = FlowState::new(return_to, unix_now());
            let mut url = discovery.authorization_endpoint.clone();
            url.query_pairs_mut()
                .append_pair("response_type", "code")
                .append_pair("client_id", &self.client_id)
                .append_pair("redirect_uri", &self.redirect_uri)
                .append_pair("scope", &self.scope)
                .append_pair("state", &flow.state)
                .append_pair("nonce", &flow.nonce)
                .append_pair("code_challenge", &flow::pkce_challenge(&flow.verifier))
                .append_pair("code_challenge_method", "S256");
            Ok(Begin {
                redirect_to: url.to_string(),
                flow_cookie: flow.seal(&self.cipher),
            })
        })
    }

    fn return_to_of(&self, flow_cookie: &str) -> Option<String> {
        FlowState::return_to_of(&self.cipher, flow_cookie)
    }

    fn complete<'a>(
        &'a self,
        params: &'a CallbackParams,
        flow_cookie: &'a str,
    ) -> BoxFuture<'a, Result<Completed, ExternalError>> {
        Box::pin(async move {
            // The provider's refusal is told by its code alone; its
            // description is text an attacker could have chosen.
            if let Some(error) = params.get("error") {
                return Err(ExternalError::IdpError(clean_error_code(error)));
            }
            let now = unix_now();
            let flow = FlowState::open(&self.cipher, flow_cookie, now)?;
            if !flow.state_matches(params.get("state")) {
                return Err(ExternalError::BadState);
            }
            let code = params
                .get("code")
                .filter(|c| !c.is_empty())
                .ok_or_else(|| ExternalError::Exchange("the callback holds no code".to_string()))?;

            let discovery = self.discovery.get().await?;
            let (id_token, access_token) = self.exchange(&discovery, code, &flow.verifier).await?;
            let jwks = self.jwks_for(&discovery);
            let id = token::validate(
                &id_token,
                &jwks,
                &Expected {
                    issuer: &self.issuer,
                    client_id: &self.client_id,
                    nonce: &flow.nonce,
                    now,
                },
            )
            .await?;
            let info = match (
                id.text("email"),
                discovery.userinfo_endpoint.as_ref(),
                access_token.as_deref(),
            ) {
                (None, Some(endpoint), Some(access)) => {
                    Some(self.user_info(endpoint, access, &id.subject).await?)
                }
                _ => None,
            };
            let identity = identity_from(&self.issuer, &id, info.as_ref(), &self.groups_claim)?;
            Ok(Completed {
                identity,
                return_to: flow.return_to,
            })
        })
    }
}

/// Makes the identity from the ID token's claims, and from the userinfo
/// answer when the token held no email.
///
/// `email_verified`: the claim, when the provider sends one (a boolean or
/// the text `true`/`false`). A missing claim counts as false, except for
/// Entra ID: its tokens usually omit the claim, so for an issuer under
/// `https://login.microsoftonline.com/` the email is taken as verified when
/// it comes from the ID token's own `email` claim and the token's `tid`
/// (tenant) is the tenant named in the issuer, i.e. the tenant's directory
/// vouches for the address. An address from the userinfo endpoint never
/// gets this.
fn identity_from(
    issuer: &str,
    id: &Claims,
    user_info: Option<&Claims>,
    groups_claim: &str,
) -> Result<ExternalIdentity, ExternalError> {
    let from_token = id.text("email");
    let source = if from_token.is_some() {
        id
    } else {
        user_info.unwrap_or(id)
    };
    let raw_email = from_token
        .clone()
        .or_else(|| user_info.and_then(|i| i.text("email")));
    let email = raw_email
        .as_deref()
        .and_then(|e| super::normalize_email(e).ok())
        .ok_or_else(|| {
            ExternalError::Token("the provider gave no valid email address".to_string())
        })?;
    let email_verified = source.flag("email_verified").unwrap_or_else(|| {
        let tenant = issuer
            .strip_prefix(ENTRA_ISSUER_PREFIX)
            .and_then(|rest| rest.split('/').next())
            .filter(|t| !t.is_empty());
        match (tenant, id.text("tid"), from_token.is_some()) {
            (Some(tenant), Some(tid), true) => tid.eq_ignore_ascii_case(tenant),
            _ => false,
        }
    });
    let groups = if groups_claim.is_empty() {
        None
    } else {
        id.strings_if_present(groups_claim)
    };
    Ok(ExternalIdentity {
        provider: "oidc",
        external_id: format!("{issuer}|{}", id.subject),
        email,
        email_verified,
        name: id
            .text("name")
            .or_else(|| user_info.and_then(|i| i.text("name"))),
        groups,
    })
}

#[cfg(test)]
mod tests {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use jsonwebtoken::Algorithm;
    use serde_json::json;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::testkit::test_keys;
    use super::*;

    const REDIRECT: &str = "http://localhost:3000/api/auth/oidc/callback";

    fn cipher() -> Cipher {
        Cipher::from_hex(&"ab".repeat(32)).unwrap()
    }

    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    struct Idp {
        server: MockServer,
        provider: OidcProvider,
    }

    async fn idp_with(auth_methods: Option<Value>, scopes: &str) -> Idp {
        let server = MockServer::start().await;
        let mut doc = json!({
            "issuer": server.uri(),
            "authorization_endpoint": format!("{}/authorize?prompt=select", server.uri()),
            "token_endpoint": format!("{}/token", server.uri()),
            "jwks_uri": format!("{}/jwks", server.uri()),
            "userinfo_endpoint": format!("{}/userinfo", server.uri()),
        });
        if let Some(methods) = auth_methods {
            doc["token_endpoint_auth_methods_supported"] = methods;
        }
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(doc))
            .mount(&server)
            .await;
        let keys = test_keys();
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"keys": [keys.rsa_jwk("k1")]})),
            )
            .mount(&server)
            .await;
        let settings = crate::store::OidcSettings {
            issuer: server.uri(),
            client_id: "client-1".into(),
            scopes: scopes.into(),
            ..crate::store::OidcSettings::default()
        };
        let provider = OidcProvider::new(
            &settings,
            "s3cret/+",
            REDIRECT,
            &crate::app::http_client(),
            &cipher(),
        );
        Idp { server, provider }
    }

    async fn idp() -> Idp {
        idp_with(None, "").await
    }

    impl Idp {
        fn claims(&self, nonce: &str) -> Value {
            json!({
                "iss": self.server.uri(), "aud": "client-1", "sub": "sub-1",
                "nonce": nonce, "iat": now(), "exp": now() + 600,
                "email": "Ann@Example.com", "email_verified": true, "name": "Ann",
                "groups": ["eng", "admins"],
            })
        }

        async fn token_endpoint(&self, flow: &FlowState, claims: Value, extra: Value) {
            let id_token = test_keys().sign(Algorithm::RS256, Some("k1"), &claims);
            let mut body =
                json!({"id_token": id_token, "access_token": "at-1", "token_type": "Bearer"});
            for (k, v) in extra.as_object().cloned().unwrap_or_default() {
                body[k] = v;
            }
            Mock::given(method("POST"))
                .and(path("/token"))
                .and(body_string_contains("grant_type=authorization_code"))
                .and(body_string_contains("code=the-code"))
                .and(body_string_contains(format!(
                    "code_verifier={}",
                    flow.verifier
                )))
                .and(body_string_contains(
                    "redirect_uri=http%3A%2F%2Flocalhost%3A3000%2Fapi%2Fauth%2Foidc%2Fcallback",
                ))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&self.server)
                .await;
        }
    }

    fn callback(flow: &FlowState) -> CallbackParams {
        [("code", "the-code"), ("state", flow.state.as_str())]
            .into_iter()
            .collect()
    }

    async fn begun(idp: &Idp) -> (Begin, FlowState) {
        let begin = idp.provider.begin("/keys?tab=1").await.unwrap();
        let flow = FlowState::open(&cipher(), &begin.flow_cookie, now()).unwrap();
        (begin, flow)
    }

    #[tokio::test]
    async fn begin_builds_the_authorization_request() {
        let idp = idp_with(None, "offline_access email groups").await;
        let (begin, flow) = begun(&idp).await;
        assert_eq!(flow.return_to, "/keys?tab=1");
        let url = Url::parse(&begin.redirect_to).unwrap();
        assert_eq!(url.path(), "/authorize");
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["prompt"], "select", "the endpoint's own query is kept");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], "client-1");
        assert_eq!(q["redirect_uri"], REDIRECT);
        assert_eq!(q["scope"], "openid email profile offline_access groups");
        assert_eq!(q["state"], flow.state);
        assert_eq!(q["nonce"], flow.nonce);
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"], flow::pkce_challenge(&flow.verifier));
        assert!(!begin.redirect_to.contains(&flow.verifier));
        assert!(!begin.redirect_to.contains("s3cret"));
        let (_, again) = begun(&idp).await;
        assert_ne!(again.state, flow.state);
    }

    #[tokio::test]
    async fn complete_exchanges_the_code_and_returns_the_identity() {
        let idp = idp().await;
        let (begin, flow) = begun(&idp).await;
        idp.token_endpoint(&flow, idp.claims(&flow.nonce), json!({}))
            .await;
        // client_secret_basic, the secret form-encoded first.
        let basic = STANDARD.encode("client-1:s3cret%2F%2B");
        let who = idp
            .provider
            .complete(&callback(&flow), &begin.flow_cookie)
            .await
            .unwrap();
        assert_eq!(who.return_to, "/keys?tab=1");
        let who = who.identity;
        assert_eq!(who.provider, "oidc");
        assert_eq!(who.external_id, format!("{}|sub-1", idp.server.uri()));
        assert_eq!(who.email, "ann@example.com");
        assert!(who.email_verified);
        assert_eq!(who.name.as_deref(), Some("Ann"));
        assert_eq!(
            who.groups,
            Some(vec!["eng".to_string(), "admins".to_string()])
        );
        let requests = idp.server.received_requests().await.unwrap();
        let token_request = requests.iter().find(|r| r.url.path() == "/token").unwrap();
        let auth = token_request
            .headers
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth, format!("Basic {basic}"));
        let body = String::from_utf8_lossy(&token_request.body).to_string();
        assert!(
            !body.contains("client_secret"),
            "basic auth: the secret is not in the body"
        );
    }

    #[tokio::test]
    async fn the_secret_goes_in_the_body_when_basic_is_not_offered() {
        let idp = idp_with(Some(json!(["client_secret_post"])), "").await;
        let (begin, flow) = begun(&idp).await;
        idp.token_endpoint(&flow, idp.claims(&flow.nonce), json!({}))
            .await;
        idp.provider
            .complete(&callback(&flow), &begin.flow_cookie)
            .await
            .unwrap();
        let requests = idp.server.received_requests().await.unwrap();
        let token_request = requests.iter().find(|r| r.url.path() == "/token").unwrap();
        assert!(token_request.headers.get("authorization").is_none());
        let body = String::from_utf8_lossy(&token_request.body).to_string();
        assert!(body.contains("client_id=client-1"));
        assert!(body.contains("client_secret=s3cret%2F%2B"));
    }

    #[tokio::test]
    async fn state_and_cookie_problems_are_refused_before_any_request() {
        let idp = idp().await;
        let (begin, flow) = begun(&idp).await;
        let before = idp.server.received_requests().await.unwrap().len();

        let wrong: CallbackParams = [("code", "the-code"), ("state", "other")]
            .into_iter()
            .collect();
        assert_eq!(
            idp.provider.complete(&wrong, &begin.flow_cookie).await,
            Err(ExternalError::BadState)
        );
        let none: CallbackParams = [("code", "the-code")].into_iter().collect();
        assert_eq!(
            idp.provider.complete(&none, &begin.flow_cookie).await,
            Err(ExternalError::BadState)
        );
        assert_eq!(
            idp.provider.complete(&callback(&flow), "").await,
            Err(ExternalError::BadState)
        );
        let mut tampered = begin.flow_cookie.clone();
        tampered.replace_range(10..11, if &tampered[10..11] == "A" { "B" } else { "A" });
        assert_eq!(
            idp.provider.complete(&callback(&flow), &tampered).await,
            Err(ExternalError::BadState)
        );

        let mut old = FlowState::new("/", now() - 601);
        old.state = flow.state.clone();
        assert_eq!(
            idp.provider
                .complete(&callback(&flow), &old.seal(&cipher()))
                .await,
            Err(ExternalError::Expired)
        );
        let no_code: CallbackParams = [("state", flow.state.as_str())].into_iter().collect();
        // A state sent twice is ambiguous and refused.
        let twice: CallbackParams = [
            ("code", "the-code"),
            ("state", flow.state.as_str()),
            ("state", flow.state.as_str()),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            idp.provider.complete(&twice, &begin.flow_cookie).await,
            Err(ExternalError::BadState)
        );
        assert!(idp
            .provider
            .complete(&no_code, &begin.flow_cookie)
            .await
            .is_err());

        assert_eq!(idp.server.received_requests().await.unwrap().len(), before);
    }

    #[tokio::test]
    async fn a_provider_error_is_reported_by_its_code_only() {
        let idp = idp().await;
        let (begin, flow) = begun(&idp).await;
        let q: CallbackParams = [
            ("state", flow.state.as_str()),
            ("error", "access_denied"),
            (
                "error_description",
                "<script>alert(1)</script> secret words",
            ),
        ]
        .into_iter()
        .collect();
        let err = idp
            .provider
            .complete(&q, &begin.flow_cookie)
            .await
            .unwrap_err();
        assert_eq!(err, ExternalError::IdpError("access_denied".into()));
        let long = "x y<z>".repeat(50);
        let weird: CallbackParams = [("error", long.as_str())].into_iter().collect();
        let ExternalError::IdpError(code) = idp.provider.complete(&weird, "").await.unwrap_err()
        else {
            panic!("expected IdpError");
        };
        assert!(code.len() <= 64 && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }

    #[tokio::test]
    async fn exchange_failures_name_no_provider_text() {
        let idp = idp().await;
        let (begin, flow) = begun(&idp).await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(
                json!({"error": "invalid_grant", "error_description": "leaky detail"}),
            ))
            .mount(&idp.server)
            .await;
        let err = idp
            .provider
            .complete(&callback(&flow), &begin.flow_cookie)
            .await
            .unwrap_err();
        let ExternalError::Exchange(m) = &err else {
            panic!("{err:?}")
        };
        assert!(m.contains("400"));
        assert!(!format!("{err}").contains("leaky"));
    }

    #[tokio::test]
    async fn an_answer_without_an_id_token_or_with_a_bad_one_is_refused() {
        let idp = idp().await;
        let (begin, flow) = begun(&idp).await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token": "a"})))
            .up_to_n_times(1)
            .mount(&idp.server)
            .await;
        assert!(matches!(
            idp.provider
                .complete(&callback(&flow), &begin.flow_cookie)
                .await,
            Err(ExternalError::Token(_))
        ));
        // A token for another nonce (a replayed token) is refused.
        idp.token_endpoint(&flow, idp.claims("another-nonce"), json!({}))
            .await;
        assert_eq!(
            idp.provider
                .complete(&callback(&flow), &begin.flow_cookie)
                .await,
            Err(ExternalError::Token("nonce mismatch".into()))
        );
    }

    #[tokio::test]
    async fn userinfo_fills_a_missing_email_for_the_same_subject_only() {
        let idp = idp().await;
        let (begin, flow) = begun(&idp).await;
        let mut claims = idp.claims(&flow.nonce);
        claims.as_object_mut().unwrap().remove("email");
        claims.as_object_mut().unwrap().remove("email_verified");
        idp.token_endpoint(&flow, claims, json!({})).await;
        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .and(header("authorization", "Bearer at-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"sub": "sub-1", "email": "ann@example.com", "email_verified": true}),
            ))
            .up_to_n_times(1)
            .mount(&idp.server)
            .await;
        let who = idp
            .provider
            .complete(&callback(&flow), &begin.flow_cookie)
            .await
            .unwrap();
        let who = who.identity;
        assert_eq!(who.email, "ann@example.com");
        assert!(who.email_verified);

        // Another subject in the userinfo answer is refused.
        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"sub": "someone-else", "email": "admin@example.com", "email_verified": true}),
            ))
            .mount(&idp.server)
            .await;
        assert!(matches!(
            idp.provider
                .complete(&callback(&flow), &begin.flow_cookie)
                .await,
            Err(ExternalError::Token(_))
        ));
    }

    #[tokio::test]
    async fn no_email_anywhere_is_refused() {
        let idp = idp().await;
        let (begin, flow) = begun(&idp).await;
        let mut claims = idp.claims(&flow.nonce);
        claims.as_object_mut().unwrap().remove("email");
        idp.token_endpoint(&flow, claims, json!({"access_token": null}))
            .await;
        assert!(matches!(
            idp.provider.complete(&callback(&flow), &begin.flow_cookie).await,
            Err(ExternalError::Token(m)) if m.contains("email")
        ));
    }

    fn claims_of(v: Value) -> Claims {
        Claims {
            subject: "s".into(),
            raw: v.as_object().unwrap().clone(),
        }
    }

    const ENTRA: &str =
        "https://login.microsoftonline.com/11111111-2222-3333-4444-555555555555/v2.0";

    #[test]
    fn email_verified_follows_the_claim_and_the_entra_rule() {
        let who = |issuer: &str, c: Value| {
            identity_from(issuer, &claims_of(c), None, "groups")
                .unwrap()
                .email_verified
        };
        let base = json!({"email": "a@example.com"});
        // Generic issuers: a missing claim is false.
        assert!(!who("https://idp.example.com", base.clone()));
        assert!(who(
            "https://idp.example.com",
            json!({"email": "a@example.com", "email_verified": true})
        ));
        assert!(who(
            "https://idp.example.com",
            json!({"email": "a@example.com", "email_verified": "true"})
        ));
        assert!(!who(
            "https://idp.example.com",
            json!({"email": "a@example.com", "email_verified": false})
        ));
        assert!(!who(
            "https://idp.example.com",
            json!({"email": "a@example.com", "email_verified": "yes"})
        ));
        // The Entra rule applies only to the Entra issuer prefix...
        let tid = json!({"email": "a@example.com", "tid": "11111111-2222-3333-4444-555555555555"});
        assert!(!who(
            "https://idp.example.com/11111111-2222-3333-4444-555555555555/v2.0",
            tid.clone()
        ));
        assert!(!who("https://login.microsoftonline.com.evil.example/11111111-2222-3333-4444-555555555555/v2.0", tid.clone()));
        // ...and then needs a tid equal to the issuer's tenant.
        assert!(who(ENTRA, tid));
        assert!(!who(ENTRA, base.clone()));
        assert!(!who(
            ENTRA,
            json!({"email": "a@example.com", "tid": "99999999-2222-3333-4444-555555555555"})
        ));
        assert!(!who(ENTRA, json!({"email": "a@example.com", "tid": 5})));
        // An explicit false stays false; an explicit true stays true.
        assert!(!who(
            ENTRA,
            json!({"email": "a@example.com", "email_verified": false, "tid": "11111111-2222-3333-4444-555555555555"})
        ));
        assert!(who(
            ENTRA,
            json!({"email": "a@example.com", "email_verified": true})
        ));
    }

    #[test]
    fn the_entra_rule_does_not_cover_an_email_from_userinfo() {
        let id = claims_of(json!({"tid": "11111111-2222-3333-4444-555555555555"}));
        let info = claims_of(json!({"email": "a@example.com"}));
        let who = identity_from(ENTRA, &id, Some(&info), "groups").unwrap();
        assert!(!who.email_verified);
        assert_eq!(who.email, "a@example.com");
    }

    #[test]
    fn identity_normalizes_and_bounds_what_the_provider_sent() {
        let id = identity_from(
            "https://idp.example.com",
            &claims_of(json!({"email": "  Ann@Example.COM ", "name": " Ann ", "roles": "solo"})),
            None,
            "roles",
        )
        .unwrap();
        assert_eq!(id.email, "ann@example.com");
        assert_eq!(id.name.as_deref(), Some("Ann"));
        assert_eq!(id.groups, Some(vec!["solo".to_string()]));
        assert_eq!(id.external_id, "https://idp.example.com|s");
        for bad in ["not an email", "a@b", "@example.com", ""] {
            assert!(
                identity_from(
                    "https://idp.example.com",
                    &claims_of(json!({"email": bad})),
                    None,
                    "groups"
                )
                .is_err(),
                "{bad}"
            );
        }
        // No groups claim: the groups are unknown, which is not the same
        // as an empty list (a token whose claim was too large for the
        // provider to include, as Entra's "groups overage", has none).
        let groups_of = |claims: serde_json::Value, claim: &str| {
            identity_from("https://idp.example.com", &claims_of(claims), None, claim)
                .unwrap()
                .groups
        };
        let base = json!({"email": "a@example.com"});
        assert_eq!(groups_of(base.clone(), "groups"), None);
        assert_eq!(
            groups_of(json!({"email": "a@example.com", "groups": []}), "groups"),
            Some(vec![])
        );
        for odd in [json!(null), json!(7), json!({"a": 1}), json!(true)] {
            assert_eq!(
                groups_of(json!({"email": "a@example.com", "groups": odd}), "groups"),
                None,
                "{odd}"
            );
        }
        // No claim name configured: nothing to read.
        assert_eq!(
            groups_of(json!({"email": "a@example.com", "groups": ["x"]}), ""),
            None
        );
        let none =
            identity_from("https://idp.example.com", &claims_of(base), None, "groups").unwrap();
        assert_eq!(none.name, None);
    }
}

#[cfg(test)]
pub(crate) mod testkit {
    //! Keys made in the tests and tokens signed with them. Nothing here is a
    //! credential: every key is generated when the test process starts.

    use std::sync::OnceLock;

    use aws_lc_rs::encoding::{AsDer, Pkcs8V1Der};
    use aws_lc_rs::rsa::{KeyPair as RsaKeyPair, KeySize};
    use aws_lc_rs::signature::{
        EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING, ECDSA_P384_SHA384_FIXED_SIGNING,
    };
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use jsonwebtoken::{Algorithm, EncodingKey, Header};
    use serde_json::{json, Value};

    pub struct TestKeys {
        rsa: RsaKeyPair,
        other_rsa: RsaKeyPair,
        ec: (Vec<u8>, EcdsaKeyPair),
        ec384: (Vec<u8>, EcdsaKeyPair),
    }

    fn ec_pair(
        alg: &'static aws_lc_rs::signature::EcdsaSigningAlgorithm,
    ) -> (Vec<u8>, EcdsaKeyPair) {
        let rng = aws_lc_rs::rand::SystemRandom::new();
        let der = EcdsaKeyPair::generate_pkcs8(alg, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(alg, der.as_ref()).unwrap();
        (der.as_ref().to_vec(), pair)
    }

    pub fn test_keys() -> &'static TestKeys {
        static KEYS: OnceLock<TestKeys> = OnceLock::new();
        KEYS.get_or_init(|| TestKeys {
            rsa: RsaKeyPair::generate(KeySize::Rsa2048).unwrap(),
            other_rsa: RsaKeyPair::generate(KeySize::Rsa2048).unwrap(),
            ec: ec_pair(&ECDSA_P256_SHA256_FIXED_SIGNING),
            ec384: ec_pair(&ECDSA_P384_SHA384_FIXED_SIGNING),
        })
    }

    fn b64(bytes: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(bytes)
    }

    /// Reads one DER element at the start of `der`: (tag, content, rest).
    fn der_element(der: &[u8]) -> (u8, &[u8], &[u8]) {
        let (tag, len_byte) = (der[0], der[1]);
        let (len, header) = if len_byte < 0x80 {
            (usize::from(len_byte), 2)
        } else {
            let n = usize::from(len_byte & 0x7f);
            let len = der[2..2 + n]
                .iter()
                .fold(0usize, |a, b| (a << 8) | usize::from(*b));
            (len, 2 + n)
        };
        (tag, &der[header..header + len], &der[header + len..])
    }

    /// The RSA key as PKCS#1 DER, which is what the signing side of
    /// `jsonwebtoken` takes. aws-lc-rs exports PKCS#8 only, so the
    /// `RSAPrivateKey` is cut out of it: PrivateKeyInfo is
    /// SEQUENCE { version, algorithm, OCTET STRING { RSAPrivateKey } }.
    fn rsa_pkcs1(key: &RsaKeyPair) -> Vec<u8> {
        let der: Pkcs8V1Der<'static> = key.as_der().unwrap();
        let (_, info, _) = der_element(der.as_ref());
        let (_, _version, rest) = der_element(info);
        let (_, _algorithm, rest) = der_element(rest);
        let (tag, private_key, _) = der_element(rest);
        assert_eq!(tag, 0x04);
        private_key.to_vec()
    }

    /// x and y of an uncompressed point (0x04 || x || y).
    fn xy(point: &[u8]) -> (String, String) {
        let half = (point.len() - 1) / 2;
        (b64(&point[1..=half]), b64(&point[1 + half..]))
    }

    impl TestKeys {
        pub fn rsa_jwk(&self, kid: &str) -> Value {
            let public = self.rsa.public_key();
            json!({
                "kty": "RSA", "kid": kid, "use": "sig",
                "n": b64(public.modulus().big_endian_without_leading_zero()),
                "e": b64(public.exponent().big_endian_without_leading_zero()),
            })
        }

        pub fn ec_jwk(&self, kid: &str) -> Value {
            let (x, y) = xy(self.ec.1.public_key().as_ref());
            json!({"kty": "EC", "crv": "P-256", "kid": kid, "x": x, "y": y})
        }

        pub fn ec384_jwk(&self, kid: &str) -> Value {
            let (x, y) = xy(self.ec384.1.public_key().as_ref());
            json!({"kty": "EC", "crv": "P-384", "kid": kid, "x": x, "y": y})
        }

        fn encoding_key(&self, alg: Algorithm) -> EncodingKey {
            match alg {
                Algorithm::ES256 => EncodingKey::from_ec_der(&self.ec.0),
                Algorithm::ES384 => EncodingKey::from_ec_der(&self.ec384.0),
                _ => EncodingKey::from_rsa_der(&rsa_pkcs1(&self.rsa)),
            }
        }

        pub fn sign(&self, alg: Algorithm, kid: Option<&str>, claims: &Value) -> String {
            let mut header = Header::new(alg);
            header.kid = kid.map(str::to_string);
            jsonwebtoken::encode(&header, claims, &self.encoding_key(alg)).unwrap()
        }

        /// Signed with a key the provider does not publish.
        pub fn sign_with_other_key(&self, alg: Algorithm, kid: &str, claims: &Value) -> String {
            let mut header = Header::new(alg);
            header.kid = Some(kid.to_string());
            let key = EncodingKey::from_rsa_der(&rsa_pkcs1(&self.other_rsa));
            jsonwebtoken::encode(&header, claims, &key).unwrap()
        }

        pub fn sign_hs256(&self, secret: &[u8], kid: &str, claims: &Value) -> String {
            let mut header = Header::new(Algorithm::HS256);
            header.kid = Some(kid.to_string());
            jsonwebtoken::encode(&header, claims, &EncodingKey::from_secret(secret)).unwrap()
        }

        /// The classic confusion attack: the RSA public key as the HMAC secret.
        pub fn hs256_with_public_key(&self, kid: &str, claims: &Value) -> String {
            self.sign_hs256(self.rsa.public_key().as_ref(), kid, claims)
        }

        /// A token with an unsigned `none` header.
        pub fn unsigned_none(&self, kid: Option<&str>, claims: &Value) -> String {
            self.fake_alg_inner("none", kid, claims, "")
        }

        /// A token whose header names `alg` and whose signature is junk.
        pub fn fake_alg(&self, alg: &str, kid: &str, claims: &Value) -> String {
            self.fake_alg_inner(alg, Some(kid), claims, "c2lnbmF0dXJl")
        }

        fn fake_alg_inner(
            &self,
            alg: &str,
            kid: Option<&str>,
            claims: &Value,
            sig: &str,
        ) -> String {
            let header = json!({"alg": alg, "typ": "JWT", "kid": kid});
            format!(
                "{}.{}.{}",
                b64(header.to_string().as_bytes()),
                b64(claims.to_string().as_bytes()),
                sig
            )
        }
    }
}
