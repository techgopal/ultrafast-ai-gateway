//! Sign in with OpenID Connect, end to end: `/api/auth/methods`,
//! `/api/auth/oidc/start` and `/api/auth/oidc/callback` against a mock
//! identity provider on wiremock that signs ID tokens with a key made here.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aws_lc_rs::encoding::{AsDer, Pkcs8V1Der};
use aws_lc_rs::rsa::{KeyPair as RsaKeyPair, KeySize};
use aws_lc_rs::signature::KeyPair;
use axum::http::{HeaderMap, StatusCode};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use common::{email_of, error_code, org_with_public_url, send, Org, Signed, ORG_PASSWORD};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{json, Value};
use ultrafast_gateway::identity::oidc::flow::FlowState;
use ultrafast_gateway::identity::{Role, UserStatus};
use ultrafast_gateway::store::NewUser;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CLIENT_ID: &str = "gateway-client";
const CLIENT_SECRET: &str = "client-secret-never-logged-91c7";
const ACCESS_TOKEN: &str = "access-token-never-logged-5e1d";
const CODE: &str = "authorization-code-never-logged-77ab";
const PUBLIC_URL: &str = "https://gateway.example.com";

// ---------------------------------------------------------------- the IdP

fn rsa_key() -> &'static RsaKeyPair {
    static KEY: OnceLock<RsaKeyPair> = OnceLock::new();
    KEY.get_or_init(|| RsaKeyPair::generate(KeySize::Rsa2048).unwrap())
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

/// The RSAPrivateKey inside the PKCS#8 export, which is what the signing
/// side of `jsonwebtoken` takes.
fn rsa_pkcs1() -> Vec<u8> {
    let der: Pkcs8V1Der<'static> = rsa_key().as_der().unwrap();
    let (_, info, _) = der_element(der.as_ref());
    let (_, _version, rest) = der_element(info);
    let (_, _algorithm, rest) = der_element(rest);
    let (tag, private_key, _) = der_element(rest);
    assert_eq!(tag, 0x04);
    private_key.to_vec()
}

fn unix_now() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap()
}

struct Idp {
    server: MockServer,
    issuer: String,
    /// What the token endpoint answers with, when set.
    id_token: Mutex<Option<String>>,
}

impl Idp {
    async fn start() -> Idp {
        let server = MockServer::start().await;
        let issuer = server.uri();
        let idp = Idp {
            server,
            issuer,
            id_token: Mutex::new(None),
        };
        idp.mount().await;
        idp
    }

    async fn mount(&self) {
        self.server.reset().await;
        let public = rsa_key().public_key();
        let b64 = |b: &[u8]| URL_SAFE_NO_PAD.encode(b);
        let jwk = json!({
            "kty": "RSA", "kid": "k1", "use": "sig",
            "n": b64(public.modulus().big_endian_without_leading_zero()),
            "e": b64(public.exponent().big_endian_without_leading_zero()),
        });
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "issuer": self.issuer,
                "authorization_endpoint": format!("{}/authorize", self.issuer),
                "token_endpoint": format!("{}/token", self.issuer),
                "jwks_uri": format!("{}/jwks", self.issuer),
                "token_endpoint_auth_methods_supported": ["client_secret_basic"],
            })))
            .mount(&self.server)
            .await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "keys": [jwk] })))
            .mount(&self.server)
            .await;
        let token = self.id_token.lock().unwrap().clone();
        if let Some(token) = token {
            Mock::given(method("POST"))
                .and(path("/token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "id_token": token,
                    "access_token": ACCESS_TOKEN,
                    "token_type": "Bearer",
                })))
                .mount(&self.server)
                .await;
        }
    }

    fn sign(&self, claims: &Value) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("k1".to_string());
        jsonwebtoken::encode(&header, claims, &EncodingKey::from_rsa_der(&rsa_pkcs1())).unwrap()
    }

    /// The token endpoint answers with an ID token for these claims.
    async fn token_for(&self, claims: &Value) {
        *self.id_token.lock().unwrap() = Some(self.sign(claims));
        self.mount().await;
    }

    async fn token_requests(&self) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path() == "/token")
            .count()
    }
}

/// The claims of a good ID token for `sub`, for the attempt `nonce` belongs
/// to. `extra` overrides keys; a `null` removes one.
fn claims(idp: &Idp, nonce: &str, sub: &str, email: &str, extra: Value) -> Value {
    let now = unix_now();
    let mut c = json!({
        "iss": idp.issuer, "aud": CLIENT_ID, "sub": sub, "email": email,
        "email_verified": true, "name": "Test Person", "nonce": nonce,
        "iat": now, "exp": now + 600,
    });
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        if v.is_null() {
            c.as_object_mut().unwrap().remove(&k);
        } else {
            c[k] = v;
        }
    }
    c
}

// ------------------------------------------------------------- the driver

async fn configure(org: &Org, idp: &Idp, extra: Value) {
    let mut body = json!({
        "enabled": true, "label": "Test IdP", "issuer": idp.issuer,
        "client_id": CLIENT_ID, "client_secret": CLIENT_SECRET,
        "scopes": "", "groups_claim": "groups", "admin_group": "",
        "link_by_email": true, "auto_create": false, "allowed_domains": [],
    });
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        body[k] = v;
    }
    let maya = org.sign_in("maya").await;
    let (status, saved) = org
        .call(Some(&maya), "PUT", "/api/settings/oidc", Some(body))
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
}

/// An organization with single sign-on on, and the IdP behind it.
async fn world() -> (Org, Idp) {
    let org = org_with_public_url(PUBLIC_URL).await;
    let idp = Idp::start().await;
    configure(&org, &idp, json!({})).await;
    (org, idp)
}

fn set_cookies(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect()
}

/// What the gateway answered to a browser request.
#[derive(Debug)]
struct Answer {
    status: StatusCode,
    location: String,
    cookies: Vec<String>,
}

impl Answer {
    fn of(status: StatusCode, headers: &HeaderMap) -> Answer {
        Answer {
            status,
            location: headers
                .get("location")
                .map(|v| v.to_str().unwrap().to_string())
                .unwrap_or_default(),
            cookies: set_cookies(headers),
        }
    }

    /// The `name=value` pair of a cookie that was set to a value.
    fn cookie(&self, name: &str) -> Option<String> {
        self.cookies
            .iter()
            .map(|c| c.split(';').next().unwrap().trim().to_string())
            .find(|p| p.starts_with(&format!("{name}=")) && !p.ends_with('='))
    }

    fn session(&self) -> Option<String> {
        self.cookie("uf_session")
    }

    /// The whole `Set-Cookie` text of a cookie.
    fn set_cookie(&self, name: &str) -> Option<&String> {
        self.cookies
            .iter()
            .find(|c| c.starts_with(&format!("{name}=")))
    }

    fn clears_flow_cookie(&self) -> bool {
        self.set_cookie("uf_oidc")
            .is_some_and(|c| c.starts_with("uf_oidc=;") && c.contains("Max-Age=0"))
    }

    /// Sign-in was refused with this code: back to the sign-in page, no
    /// session, the flow cookie gone.
    fn assert_refused(&self, code: &str) {
        assert_eq!(self.status, StatusCode::FOUND, "{self:?}");
        assert_eq!(
            self.location,
            format!("/sign-in?sso_error={code}"),
            "{self:?}"
        );
        assert!(self.session().is_none(), "a session was made: {self:?}");
        assert!(self.clears_flow_cookie(), "{self:?}");
    }

    fn assert_signed_in(&self, to: &str) {
        assert_eq!(self.status, StatusCode::FOUND, "{self:?}");
        assert_eq!(self.location, to, "{self:?}");
        assert!(self.session().is_some(), "{self:?}");
        assert!(self.clears_flow_cookie(), "{self:?}");
    }
}

struct Flow {
    state: String,
    nonce: String,
    /// `uf_oidc=<value>`
    cookie: String,
    location: String,
}

async fn begin(org: &Org, return_to: Option<&str>) -> (Answer, Option<Flow>) {
    let path = match return_to {
        Some(r) => {
            let mut url = reqwest::Url::parse("http://x/api/auth/oidc/start").unwrap();
            url.query_pairs_mut().append_pair("return_to", r);
            format!("{}?{}", url.path(), url.query().unwrap())
        }
        None => "/api/auth/oidc/start".to_string(),
    };
    let (status, headers, _) = send(&org.api.app, "GET", &path, &[], None).await;
    let answer = Answer::of(status, &headers);
    if status != StatusCode::FOUND || answer.location.starts_with("/sign-in") {
        return (answer, None);
    }
    let url = reqwest::Url::parse(&answer.location).unwrap();
    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let flow = Flow {
        state: query["state"].clone(),
        nonce: query["nonce"].clone(),
        cookie: answer.cookie("uf_oidc").expect("the flow cookie"),
        location: answer.location.clone(),
    };
    (answer, Some(flow))
}

async fn callback(org: &Org, query: &str, cookie: Option<&str>) -> Answer {
    let headers: Vec<(&str, &str)> = cookie.map(|c| ("cookie", c)).into_iter().collect();
    let (status, headers, _) = send(
        &org.api.app,
        "GET",
        &format!("/api/auth/oidc/callback?{query}"),
        &headers,
        None,
    )
    .await;
    Answer::of(status, &headers)
}

/// The whole round trip for `sub`/`email`; `extra` changes the claims.
async fn sign_in_as(
    org: &Org,
    idp: &Idp,
    return_to: Option<&str>,
    sub: &str,
    email: &str,
    extra: Value,
) -> Answer {
    let (_, flow) = begin(org, return_to).await;
    let flow = flow.expect("start must redirect");
    idp.token_for(&claims(idp, &flow.nonce, sub, email, extra))
        .await;
    callback(
        org,
        &format!("code={CODE}&state={}", flow.state),
        Some(&flow.cookie),
    )
    .await
}

/// A browser session from the answer of a successful callback.
async fn signed_from(org: &Org, answer: &Answer) -> Signed {
    let cookie = answer.session().expect("a session cookie");
    let (status, _, me) = send(
        &org.api.app,
        "GET",
        "/api/auth/me",
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{me}");
    Signed {
        cookie,
        csrf: me["csrf_token"].as_str().unwrap().to_string(),
        user_id: me["user"]["id"].as_i64().unwrap(),
    }
}

async fn user_of(org: &Org, name: &str) -> ultrafast_gateway::store::UserRow {
    org.api
        .store
        .user_by_email(&email_of(name))
        .await
        .unwrap()
        .unwrap()
}

async fn count_users(org: &Org) -> i64 {
    org.api.store.count_users().await.unwrap()
}

fn metric(org: &Org, result: &str) -> u64 {
    let text = org.api.state.metrics.render(&[]);
    let prefix = format!("uf_oidc_signins_total{{result=\"{result}\"}} ");
    text.lines()
        .find_map(|l| l.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("no uf_oidc_signins_total for {result}"))
        .parse()
        .unwrap()
}

// ------------------------------------------------------------------ tests

#[tokio::test]
async fn methods_list_password_and_the_configured_provider() {
    let org = org_with_public_url(PUBLIC_URL).await;
    let (status, _, off) = send(&org.api.app, "GET", "/api/auth/methods", &[], None).await;
    assert_eq!(status, StatusCode::OK, "{off}");
    assert_eq!(off, json!({ "password": true, "oidc": null }));

    let idp = Idp::start().await;
    configure(&org, &idp, json!({})).await;
    let (_, _, on) = send(&org.api.app, "GET", "/api/auth/methods", &[], None).await;
    assert_eq!(
        on,
        json!({ "password": true, "oidc": { "label": "Test IdP" } })
    );

    // Off again: back to the password alone.
    configure(&org, &idp, json!({ "enabled": false })).await;
    let (_, _, off) = send(&org.api.app, "GET", "/api/auth/methods", &[], None).await;
    assert_eq!(off, json!({ "password": true, "oidc": null }));
}

#[tokio::test]
async fn start_sends_the_browser_to_the_idp_with_a_flow_cookie() {
    let (org, idp) = world().await;
    let (answer, flow) = begin(&org, Some("/keys")).await;
    let flow = flow.unwrap();
    assert_eq!(answer.status, StatusCode::FOUND);

    let url = reqwest::Url::parse(&flow.location).unwrap();
    assert_eq!(
        format!("{}{}", url.origin().ascii_serialization(), url.path()),
        format!("{}/authorize", idp.issuer)
    );
    let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(q["response_type"], "code");
    assert_eq!(q["client_id"], CLIENT_ID);
    assert_eq!(
        q["redirect_uri"],
        format!("{PUBLIC_URL}/api/auth/oidc/callback")
    );
    assert_eq!(q["code_challenge_method"], "S256");
    assert!(q["scope"].starts_with("openid email profile"));
    assert!(q["state"].len() >= 43 && q["nonce"].len() >= 43);

    let cookie = answer.set_cookie("uf_oidc").unwrap();
    assert!(
        cookie.ends_with("; HttpOnly; SameSite=Lax; Path=/api/auth/oidc; Max-Age=600"),
        "{cookie}"
    );
    // The value is not the state in the clear.
    assert!(!flow.cookie.contains(&q["state"]));
}

#[tokio::test]
async fn the_flow_cookie_is_secure_where_session_cookies_are() {
    let api = common::api_tweaked(
        ultrafast_gateway::store::Store::open_in_memory()
            .await
            .unwrap(),
        |s| {
            s.cookie_secure = true;
            s.public_url = Some(PUBLIC_URL.parse().unwrap());
        },
    )
    .await;
    common::seed_user(&api.store, "root@example.com", Role::Admin, ORG_PASSWORD).await;
    let idp = Idp::start().await;
    let root = common::sign_in(&api.app, "root@example.com", ORG_PASSWORD).await;
    let (status, _, body) = common::call(
        &api.app,
        "PUT",
        "/api/settings/oidc",
        Some(&root),
        Some(json!({
            "enabled": true, "issuer": idp.issuer, "client_id": CLIENT_ID,
            "client_secret": CLIENT_SECRET,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, headers, _) = send(&api.app, "GET", "/api/auth/oidc/start", &[], None).await;
    assert_eq!(status, StatusCode::FOUND);
    let cookie = set_cookies(&headers).remove(0);
    assert!(
        cookie.ends_with("; HttpOnly; SameSite=Lax; Path=/api/auth/oidc; Max-Age=600; Secure"),
        "{cookie}"
    );
}

#[tokio::test]
async fn a_verified_email_links_the_user_and_signs_in_like_a_password() {
    let (org, idp) = world().await;
    let answer = sign_in_as(
        &org,
        &idp,
        Some("/keys"),
        "sub-priya",
        &email_of("priya"),
        json!({}),
    )
    .await;
    answer.assert_signed_in("/keys");
    assert_eq!(idp.token_requests().await, 1);
    assert!(org
        .last_summary("auth.login")
        .await
        .contains("via OIDC (Test IdP)"));

    // Linked.
    let priya = user_of(&org, "priya").await;
    assert_eq!(priya.auth_provider, "oidc");
    assert_eq!(
        priya.external_id.as_deref(),
        Some(format!("{}|sub-priya", idp.issuer).as_str())
    );
    assert_eq!(priya.status, UserStatus::Active);
    assert_eq!(priya.role, Role::Member);

    // The session cookie has the flags the password cookie has.
    let (_, headers, _) = send(
        &org.api.app,
        "POST",
        "/api/auth/login",
        &[],
        Some(
            serde_json::to_vec(&json!({"email": email_of("lena"), "password": ORG_PASSWORD}))
                .unwrap(),
        ),
    )
    .await;
    let password_cookie = Answer::of(StatusCode::OK, &headers);
    let attributes = |c: &Answer| {
        c.set_cookie("uf_session")
            .unwrap()
            .split_once(';')
            .unwrap()
            .1
            .to_string()
    };
    assert_eq!(attributes(&answer), attributes(&password_cookie));

    // It works for /api, with the CSRF token like any session.
    let signed = signed_from(&org, &answer).await;
    assert_eq!(signed.user_id, priya.id);
    let (status, body) = org
        .call(
            Some(&signed),
            "POST",
            "/api/keys",
            Some(json!({"name": "k"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, _, body) = send(
        &org.api.app,
        "POST",
        "/api/keys",
        &[("cookie", &signed.cookie)],
        Some(serde_json::to_vec(&json!({"name": "k2"})).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "csrf_failed");

    // Audited, and told in the users API.
    let actions = org.audit_actions().await;
    assert!(
        actions.contains(&"user.link_oidc".to_string()),
        "{actions:?}"
    );
    let maya = org.sign_in("maya").await;
    let (_, view) = org
        .call(
            Some(&maya),
            "GET",
            &format!("/api/users/{}", priya.id),
            None,
        )
        .await;
    assert_eq!(view["auth_provider"], "oidc");
    let (_, view) = org
        .call(
            Some(&maya),
            "GET",
            &format!("/api/users/{}", org.lena),
            None,
        )
        .await;
    assert_eq!(view["auth_provider"], "password");
    assert_eq!(metric(&org, "ok"), 1);
}

#[tokio::test]
async fn a_linked_user_is_found_by_subject_even_when_the_email_changed() {
    let (org, idp) = world().await;
    let first = sign_in_as(&org, &idp, None, "sub-priya", &email_of("priya"), json!({})).await;
    first.assert_signed_in("/");
    let before = count_users(&org).await;

    let second = sign_in_as(
        &org,
        &idp,
        None,
        "sub-priya",
        "priya.renamed@example.com",
        json!({}),
    )
    .await;
    second.assert_signed_in("/");
    assert_eq!(signed_from(&org, &second).await.user_id, org.priya);
    assert_eq!(count_users(&org).await, before);
    // The gateway's own record of the email is not rewritten.
    assert_eq!(user_of(&org, "priya").await.id, org.priya);
}

#[tokio::test]
async fn an_invited_user_becomes_active() {
    let (org, idp) = world().await;
    let mut tx = org.api.store.begin().await.unwrap();
    let sam = tx
        .insert_user(NewUser {
            email: "sam@example.com",
            name: "Sam",
            role: Role::Member,
            status: UserStatus::Invited,
            password_hash: None,
        })
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let answer = sign_in_as(&org, &idp, None, "sub-sam", "sam@example.com", json!({})).await;
    answer.assert_signed_in("/");
    let sam = org.api.store.user_by_id(sam).await.unwrap().unwrap();
    assert_eq!(sam.status, UserStatus::Active);
    assert_eq!(sam.auth_provider, "oidc");
}

#[tokio::test]
async fn users_are_created_on_first_sign_in_only_for_allowed_domains() {
    let (org, idp) = world().await;
    configure(
        &org,
        &idp,
        json!({ "auto_create": true, "allowed_domains": ["corp.example.org"] }),
    )
    .await;
    let before = count_users(&org).await;

    let refused = sign_in_as(&org, &idp, None, "sub-x", "x@evil.example.net", json!({})).await;
    refused.assert_refused("not_allowed");
    // An address the provider has not verified never makes an account.
    let unverified = sign_in_as(
        &org,
        &idp,
        None,
        "sub-u",
        "u@corp.example.org",
        json!({ "email_verified": false }),
    )
    .await;
    unverified.assert_refused("not_allowed");
    assert_eq!(count_users(&org).await, before);

    let made = sign_in_as(
        &org,
        &idp,
        None,
        "sub-new",
        "Newbie@Corp.Example.org",
        json!({ "name": "Nia Newbie" }),
    )
    .await;
    made.assert_signed_in("/");
    let user = org
        .api
        .store
        .user_by_email("newbie@corp.example.org")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(user.role, Role::Member);
    assert_eq!(user.status, UserStatus::Active);
    assert_eq!(user.name, "Nia Newbie");
    assert_eq!(user.auth_provider, "oidc");
    assert!(user.password_hash.is_none());
    assert_eq!(count_users(&org).await, before + 1);
    assert!(org
        .audit_actions()
        .await
        .contains(&"user.create_oidc".to_string()));
    assert_eq!(metric(&org, "not_allowed"), 2);
}

#[tokio::test]
async fn an_unverified_email_cannot_take_over_an_admin() {
    let (org, idp) = world().await;
    for extra in [
        json!({ "email_verified": false }),
        json!({ "email_verified": null }),
        json!({ "email_verified": "false" }),
    ] {
        let answer = sign_in_as(&org, &idp, None, "sub-attacker", &email_of("maya"), extra).await;
        answer.assert_refused("not_allowed");
    }
    let maya = user_of(&org, "maya").await;
    assert_eq!(maya.auth_provider, "password");
    assert!(maya.external_id.is_none());
    assert_eq!(maya.role, Role::Admin);

    // The same with auto-create on: the address is taken, nothing is made.
    configure(
        &org,
        &idp,
        json!({ "auto_create": true, "allowed_domains": ["example.com"] }),
    )
    .await;
    let answer = sign_in_as(
        &org,
        &idp,
        None,
        "sub-attacker",
        &email_of("maya"),
        json!({ "email_verified": false }),
    )
    .await;
    answer.assert_refused("not_allowed");

    // Link by email off: not even a verified address links.
    configure(&org, &idp, json!({ "link_by_email": false })).await;
    let answer = sign_in_as(&org, &idp, None, "sub-maya", &email_of("maya"), json!({})).await;
    answer.assert_refused("not_allowed");
    assert_eq!(user_of(&org, "maya").await.auth_provider, "password");
}

#[tokio::test]
async fn a_disabled_user_is_refused() {
    let (org, idp) = world().await;
    // Linked first, then disabled: found by subject.
    sign_in_as(&org, &idp, None, "sub-lena", &email_of("lena"), json!({}))
        .await
        .assert_signed_in("/");
    let mut tx = org.api.store.begin().await.unwrap();
    tx.set_user_status(org.lena, UserStatus::Disabled)
        .await
        .unwrap();
    tx.set_user_status(org.tomas, UserStatus::Disabled)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    sign_in_as(&org, &idp, None, "sub-lena", &email_of("lena"), json!({}))
        .await
        .assert_refused("disabled");
    // Not linked yet: found by email.
    sign_in_as(&org, &idp, None, "sub-tomas", &email_of("tomas"), json!({}))
        .await
        .assert_refused("disabled");
    assert_eq!(user_of(&org, "tomas").await.auth_provider, "password");
    assert_eq!(metric(&org, "disabled"), 2);
}

#[tokio::test]
async fn the_admin_group_promotes_and_demotes() {
    let (org, idp) = world().await;
    configure(&org, &idp, json!({ "admin_group": "gw-admins" })).await;

    sign_in_as(
        &org,
        &idp,
        None,
        "sub-priya",
        &email_of("priya"),
        json!({ "groups": ["staff", "gw-admins"] }),
    )
    .await
    .assert_signed_in("/");
    assert_eq!(user_of(&org, "priya").await.role, Role::Admin);
    assert!(org
        .audit_actions()
        .await
        .contains(&"user.role_from_idp".to_string()));

    // maya is another admin, so priya may be demoted.
    sign_in_as(
        &org,
        &idp,
        None,
        "sub-priya",
        &email_of("priya"),
        json!({ "groups": ["staff"] }),
    )
    .await
    .assert_signed_in("/");
    assert_eq!(user_of(&org, "priya").await.role, Role::Member);

    // The claim can be a single string, and the claim name is a setting.
    configure(
        &org,
        &idp,
        json!({ "admin_group": "gw-admins", "groups_claim": "roles" }),
    )
    .await;
    sign_in_as(
        &org,
        &idp,
        None,
        "sub-priya",
        &email_of("priya"),
        json!({ "roles": "gw-admins" }),
    )
    .await
    .assert_signed_in("/");
    assert_eq!(user_of(&org, "priya").await.role, Role::Admin);
}

#[tokio::test]
async fn without_an_admin_group_roles_are_not_touched() {
    let (org, idp) = world().await;
    // maya is an admin and gets no groups.
    sign_in_as(&org, &idp, None, "sub-maya", &email_of("maya"), json!({}))
        .await
        .assert_signed_in("/");
    assert_eq!(user_of(&org, "maya").await.role, Role::Admin);
    // A member with the group name in the token stays one.
    sign_in_as(
        &org,
        &idp,
        None,
        "sub-lena",
        &email_of("lena"),
        json!({ "groups": ["gw-admins"] }),
    )
    .await
    .assert_signed_in("/");
    assert_eq!(user_of(&org, "lena").await.role, Role::Member);
    assert!(!org
        .audit_actions()
        .await
        .contains(&"user.role_from_idp".to_string()));
}

#[tokio::test]
async fn the_last_admin_is_never_demoted() {
    let (org, idp) = world().await;
    configure(&org, &idp, json!({ "admin_group": "gw-admins" })).await;
    // maya is the only admin and the provider says she is in no group.
    sign_in_as(
        &org,
        &idp,
        None,
        "sub-maya",
        &email_of("maya"),
        json!({ "groups": [] }),
    )
    .await
    .assert_signed_in("/");
    assert_eq!(user_of(&org, "maya").await.role, Role::Admin);
    assert!(org
        .last_summary("user.role_from_idp")
        .await
        .contains("last active admin"));
}

#[tokio::test]
async fn a_changed_issuer_relinks_but_a_changed_subject_does_not() {
    let (org, idp) = world().await;
    // Linked under another issuer, since changed.
    let mut tx = org.api.store.begin().await.unwrap();
    tx.link_external(org.priya, "oidc", "https://old-idp.example.com|abc")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    sign_in_as(&org, &idp, None, "sub-priya", &email_of("priya"), json!({}))
        .await
        .assert_signed_in("/");
    assert_eq!(
        user_of(&org, "priya").await.external_id.as_deref(),
        Some(format!("{}|sub-priya", idp.issuer).as_str())
    );

    // Same issuer, another subject with the same email: not the same person.
    sign_in_as(
        &org,
        &idp,
        None,
        "sub-someone-else",
        &email_of("priya"),
        json!({}),
    )
    .await
    .assert_refused("not_allowed");
    assert_eq!(
        user_of(&org, "priya").await.external_id.as_deref(),
        Some(format!("{}|sub-priya", idp.issuer).as_str())
    );
}

#[tokio::test]
async fn a_wrong_state_a_missing_or_altered_cookie_and_an_old_attempt_are_refused() {
    let (org, idp) = world().await;
    let (_, flow) = begin(&org, None).await;
    let flow = flow.unwrap();
    idp.token_for(&claims(
        &idp,
        &flow.nonce,
        "s",
        &email_of("priya"),
        json!({}),
    ))
    .await;

    // Another state.
    callback(
        &org,
        &format!("code={CODE}&state=other"),
        Some(&flow.cookie),
    )
    .await
    .assert_refused("state");
    // No state, or two.
    callback(&org, &format!("code={CODE}"), Some(&flow.cookie))
        .await
        .assert_refused("state");
    callback(
        &org,
        &format!("code={CODE}&state={0}&state={0}", flow.state),
        Some(&flow.cookie),
    )
    .await
    .assert_refused("state");
    // No cookie.
    callback(&org, &format!("code={CODE}&state={}", flow.state), None)
        .await
        .assert_refused("state");
    // An altered cookie.
    let mut bytes = flow.cookie.clone().into_bytes();
    let middle = bytes.len() / 2;
    bytes[middle] = if bytes[middle] == b'A' { b'B' } else { b'A' };
    let altered = String::from_utf8(bytes).unwrap();
    callback(
        &org,
        &format!("code={CODE}&state={}", flow.state),
        Some(&altered),
    )
    .await
    .assert_refused("state");
    // A cookie that is not ours at all.
    callback(
        &org,
        &format!("code={CODE}&state={}", flow.state),
        Some("uf_oidc=not-a-flow-cookie"),
    )
    .await
    .assert_refused("state");
    // The provider was never asked for tokens.
    assert_eq!(idp.token_requests().await, 0);

    // An attempt older than ten minutes.
    let old = FlowState::new("/", unix_now() - 700);
    let cookie = format!("uf_oidc={}", old.seal(&org.api.state.cipher));
    callback(
        &org,
        &format!("code={CODE}&state={}", old.state),
        Some(&cookie),
    )
    .await
    .assert_refused("expired");
    assert_eq!(idp.token_requests().await, 0);
    assert_eq!(metric(&org, "state"), 6);
    assert_eq!(metric(&org, "expired"), 1);
}

#[tokio::test]
async fn a_callback_cannot_be_replayed() {
    let (org, idp) = world().await;
    let (_, flow) = begin(&org, None).await;
    let flow = flow.unwrap();
    idp.token_for(&claims(
        &idp,
        &flow.nonce,
        "sub-priya",
        &email_of("priya"),
        json!({}),
    ))
    .await;
    let query = format!("code={CODE}&state={}", flow.state);
    let first = callback(&org, &query, Some(&flow.cookie)).await;
    first.assert_signed_in("/");
    // The browser got the cookie cleared, so it sends none the second time.
    assert!(first.clears_flow_cookie());
    callback(&org, &query, None).await.assert_refused("state");
    assert_eq!(idp.token_requests().await, 1);
}

#[tokio::test]
async fn return_to_is_a_relative_console_path_or_the_root() {
    let (org, idp) = world().await;
    for (asked, lands) in [
        (Some("/ok?x=1"), "/ok?x=1"),
        (Some("/keys/12#top"), "/keys/12#top"),
        (Some("//evil.com"), "/"),
        (Some("//evil.com/x"), "/"),
        (Some("https://evil.com"), "/"),
        (Some("/\\evil.com"), "/"),
        (Some("\\\\evil.com"), "/"),
        (Some("/ok\r\nSet-Cookie: x=y"), "/"),
        (Some("javascript:alert(1)"), "/"),
        (Some("evil.com"), "/"),
        (Some("/api/backup"), "/"),
        (Some(""), "/"),
        (None, "/"),
    ] {
        let answer = sign_in_as(
            &org,
            &idp,
            asked,
            "sub-priya",
            &email_of("priya"),
            json!({}),
        )
        .await;
        answer.assert_signed_in(lands);
    }
    // A flow cookie that holds a bad path (it cannot be made by a browser,
    // but the check does not rely on that).
    for bad in ["//evil.com", "https://evil.com", "/\\evil.com"] {
        let flow = FlowState::new(bad, unix_now());
        idp.token_for(&claims(
            &idp,
            &flow.nonce,
            "sub-priya",
            &email_of("priya"),
            json!({}),
        ))
        .await;
        let answer = callback(
            &org,
            &format!("code={CODE}&state={}", flow.state),
            Some(&format!("uf_oidc={}", flow.seal(&org.api.state.cipher))),
        )
        .await;
        answer.assert_signed_in("/");
    }
}

#[tokio::test]
async fn a_refusal_by_the_provider_is_told_by_its_code_only() {
    let (org, idp) = world().await;
    let (_, flow) = begin(&org, None).await;
    let flow = flow.unwrap();
    let answer = callback(
        &org,
        &format!(
            "error=access_denied&error_description=%3Cscript%3Ebad%3C%2Fscript%3E&state={}",
            flow.state
        ),
        Some(&flow.cookie),
    )
    .await;
    answer.assert_refused("idp");
    assert!(!answer.location.contains("script"));
    assert_eq!(metric(&org, "idp"), 1);
    assert_eq!(idp.token_requests().await, 0);
}

#[tokio::test]
async fn a_bad_id_token_is_refused() {
    let (org, idp) = world().await;
    // Wrong nonce.
    let (_, flow) = begin(&org, None).await;
    let flow = flow.unwrap();
    idp.token_for(&claims(
        &idp,
        "not-the-nonce",
        "s",
        &email_of("priya"),
        json!({}),
    ))
    .await;
    callback(
        &org,
        &format!("code={CODE}&state={}", flow.state),
        Some(&flow.cookie),
    )
    .await
    .assert_refused("token");
    // Wrong audience.
    sign_in_as(
        &org,
        &idp,
        None,
        "s",
        &email_of("priya"),
        json!({ "aud": "someone-else" }),
    )
    .await
    .assert_refused("token");
    // Without an email.
    sign_in_as(
        &org,
        &idp,
        None,
        "s",
        &email_of("priya"),
        json!({ "email": null }),
    )
    .await
    .assert_refused("token");
    assert_eq!(metric(&org, "token"), 3);
    assert_eq!(user_of(&org, "priya").await.auth_provider, "password");
}

#[tokio::test]
async fn with_sign_in_off_start_is_missing_and_passwords_still_work() {
    let org = org_with_public_url(PUBLIC_URL).await;
    let (status, _, body) = send(&org.api.app, "GET", "/api/auth/oidc/start", &[], None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(error_code(&body), "oidc_disabled");
    callback(&org, "code=x&state=y", None)
        .await
        .assert_refused("config");
    org.sign_in("lena").await;

    // Turned off after use: the linked user keeps the password.
    let (org, idp) = world().await;
    sign_in_as(&org, &idp, None, "sub-lena", &email_of("lena"), json!({}))
        .await
        .assert_signed_in("/");
    configure(&org, &idp, json!({ "enabled": false })).await;
    let (status, _, body) = send(&org.api.app, "GET", "/api/auth/oidc/start", &[], None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    org.sign_in("lena").await;
    assert_eq!(metric(&org, "ok"), 1);
}

#[tokio::test]
async fn the_callback_is_limited_per_client_address() {
    let (org, _idp) = world().await;
    for i in 0..20 {
        callback(&org, "code=x&state=y", None)
            .await
            .assert_refused("state");
        assert_eq!(metric(&org, "state"), i + 1);
    }
    callback(&org, "code=x&state=y", None)
        .await
        .assert_refused("rate_limited");
    assert_eq!(metric(&org, "rate_limited"), 1);
}

#[tokio::test]
async fn a_successful_sign_in_gives_its_attempt_back() {
    let (org, idp) = world().await;
    for _ in 0..25 {
        sign_in_as(&org, &idp, None, "sub-priya", &email_of("priya"), json!({}))
            .await
            .assert_signed_in("/");
    }
    assert_eq!(metric(&org, "ok"), 25);
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

/// One subscriber for the whole test process: the tests run side by side,
/// and a subscriber set for one thread only is missed by call sites another
/// thread registered before it. What every test logs ends up here.
fn captured_log() -> Capture {
    static CAPTURE: OnceLock<Capture> = OnceLock::new();
    CAPTURE
        .get_or_init(|| {
            let capture = Capture::default();
            let subscriber = tracing_subscriber::fmt()
                .with_max_level(tracing::Level::TRACE)
                .with_ansi(false)
                .with_writer(capture.clone())
                .finish();
            let _ = tracing::subscriber::set_global_default(subscriber);
            capture
        })
        .clone()
}

#[tokio::test]
async fn no_token_or_secret_reaches_the_log() {
    let capture = captured_log();

    let (org, idp) = world().await;
    let (_, flow) = begin(&org, None).await;
    let flow = flow.unwrap();
    let token = idp.sign(&claims(
        &idp,
        &flow.nonce,
        "sub-priya",
        &email_of("priya"),
        json!({}),
    ));
    idp.token_for(&claims(
        &idp,
        &flow.nonce,
        "sub-priya",
        &email_of("priya"),
        json!({}),
    ))
    .await;
    let query = format!("code={CODE}&state={}", flow.state);
    let ok = callback(&org, &query, Some(&flow.cookie)).await;
    ok.assert_signed_in("/");
    callback(&org, &query, None).await.assert_refused("state");
    callback(&org, "error=access_denied&state=x", Some(&flow.cookie))
        .await
        .assert_refused("idp");
    sign_in_as(
        &org,
        &idp,
        None,
        "s",
        &email_of("priya"),
        json!({ "aud": "x" }),
    )
    .await
    .assert_refused("token");

    let log = String::from_utf8_lossy(&capture.0.lock().unwrap()).to_string();
    for reason in ["state", "idp", "token"] {
        assert!(
            log.contains(&format!(
                "single sign-on callback refused: reason=\"{reason}\""
            )) || log.contains(&format!("reason=\"{reason}\"")),
            "the refusal {reason} is logged with its code: {log}"
        );
    }
    let signature = token.rsplit('.').next().unwrap();
    for secret in [
        CODE,
        CLIENT_SECRET,
        ACCESS_TOKEN,
        signature,
        flow.cookie.trim_start_matches("uf_oidc="),
        flow.state.as_str(),
        flow.nonce.as_str(),
        ok.session().unwrap().trim_start_matches("uf_session="),
    ] {
        assert!(!log.contains(secret), "the log holds a secret: {secret}");
    }
}

#[tokio::test]
async fn a_token_without_a_groups_claim_leaves_the_role_alone() {
    let (org, idp) = world().await;
    configure(&org, &idp, json!({ "admin_group": "gw-admins" })).await;
    sign_in_as(
        &org,
        &idp,
        None,
        "sub-priya",
        &email_of("priya"),
        json!({ "groups": ["gw-admins"] }),
    )
    .await
    .assert_signed_in("/");
    assert_eq!(user_of(&org, "priya").await.role, Role::Admin);
    let audited = org.audit_actions().await.len();

    // No claim at all (a provider that left it out because it was too
    // large), or one of another kind: nothing is known, so nothing changes.
    for extra in [json!({}), json!({ "groups": null }), json!({ "groups": 7 })] {
        sign_in_as(&org, &idp, None, "sub-priya", &email_of("priya"), extra)
            .await
            .assert_signed_in("/");
        assert_eq!(user_of(&org, "priya").await.role, Role::Admin);
    }
    // And a member is not promoted by silence.
    sign_in_as(&org, &idp, None, "sub-lena", &email_of("lena"), json!({}))
        .await
        .assert_signed_in("/");
    assert_eq!(user_of(&org, "lena").await.role, Role::Member);
    let actions = org.audit_actions().await;
    assert!(
        !actions[audited..].iter().any(|a| a == "user.role_from_idp"),
        "{actions:?}"
    );

    // An empty list is an answer: priya is in no group, and maya is
    // another admin, so she is demoted.
    sign_in_as(
        &org,
        &idp,
        None,
        "sub-priya",
        &email_of("priya"),
        json!({ "groups": [] }),
    )
    .await
    .assert_signed_in("/");
    assert_eq!(user_of(&org, "priya").await.role, Role::Member);
}

#[tokio::test]
async fn starting_is_limited_per_client_address_and_a_sign_in_gives_it_back() {
    let (org, _idp) = world().await;
    for _ in 0..20 {
        let (answer, flow) = begin(&org, None).await;
        assert!(flow.is_some(), "{answer:?}");
    }
    let (answer, flow) = begin(&org, None).await;
    assert!(flow.is_none());
    assert_eq!(answer.status, StatusCode::FOUND);
    assert_eq!(answer.location, "/sign-in?sso_error=rate_limited");
    // An attempt under way keeps its cookie.
    assert!(answer.set_cookie("uf_oidc").is_none());
    // The callback shares the address bucket.
    callback(&org, "code=x&state=y", None)
        .await
        .assert_refused("rate_limited");
}
