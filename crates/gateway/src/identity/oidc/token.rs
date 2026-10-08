//! Validation of an ID token (OpenID Connect Core, section 3.1.3.7).

use jsonwebtoken::Algorithm;
use serde_json::{Map, Value};

use super::discovery::Jwks;
use crate::identity::external::ExternalError;
use crate::secrets::secrets_equal;

/// The only algorithms an ID token may be signed with. `none` and the
/// HMAC family are not here and are refused.
pub const ALLOWED: [Algorithm; 8] = [
    Algorithm::RS256,
    Algorithm::RS384,
    Algorithm::RS512,
    Algorithm::PS256,
    Algorithm::PS384,
    Algorithm::PS512,
    Algorithm::ES256,
    Algorithm::ES384,
];

/// What the token must say.
pub struct Expected<'a> {
    pub issuer: &'a str,
    pub client_id: &'a str,
    pub nonce: &'a str,
    /// Seconds since the epoch.
    pub now: i64,
}

/// The claims of an accepted token.
#[derive(Clone)]
pub struct Claims {
    pub subject: String,
    pub raw: Map<String, Value>,
}

/// Claims name a person (email, name, groups): they are never printed.
impl std::fmt::Debug for Claims {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Claims(<redacted>)")
    }
}

/// Longest ID token read (they are a few KiB).
const MAX_TOKEN_BYTES: usize = 32 * 1024;
/// How far the gateway's clock may differ from the provider's.
const LEEWAY_SECS: f64 = 60.0;
/// How far ahead of now `iat` may be, before the leeway.
const IAT_AHEAD_SECS: f64 = 300.0;
const MAX_SUBJECT_BYTES: usize = 255;
const MAX_GROUPS: usize = 1000;
const MAX_TEXT_BYTES: usize = 1024;

impl Claims {
    /// A non-empty string claim, trimmed.
    pub fn text(&self, name: &str) -> Option<String> {
        let value = self.raw.get(name)?.as_str()?.trim();
        (!value.is_empty() && value.len() <= MAX_TEXT_BYTES).then(|| value.to_string())
    }

    /// `email_verified` as a boolean; some providers send it as a string.
    pub fn flag(&self, name: &str) -> Option<bool> {
        match self.raw.get(name)? {
            Value::Bool(b) => Some(*b),
            Value::String(s) if s == "true" => Some(true),
            Value::String(s) if s == "false" => Some(false),
            _ => None,
        }
    }

    /// Strings of an array claim, or the one string of a string claim.
    /// `None` when the claim is absent or of another kind: the value is
    /// unknown, which is not the same as an empty list.
    pub fn strings_if_present(&self, name: &str) -> Option<Vec<String>> {
        match self.raw.get(name) {
            Some(Value::String(s)) => Some(vec![s.clone()]),
            Some(Value::Array(items)) => Some(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .take(MAX_GROUPS)
                    .map(str::to_string)
                    .collect(),
            ),
            _ => None,
        }
    }
}

fn refuse(reason: &str) -> ExternalError {
    ExternalError::Token(reason.to_string())
}

/// Checks the signature and every claim the flow depends on. The reasons are
/// fixed words, so they can be logged.
pub async fn validate(
    token: &str,
    jwks: &Jwks,
    expected: &Expected<'_>,
) -> Result<Claims, ExternalError> {
    if token.len() > MAX_TOKEN_BYTES {
        return Err(refuse("malformed token"));
    }
    // `none` is not an `Algorithm`: its header does not parse.
    let header = jsonwebtoken::decode_header(token).map_err(|_| refuse("malformed token"))?;
    let alg = header.alg;
    if !ALLOWED.contains(&alg) {
        return Err(refuse("algorithm not allowed"));
    }
    let key = jwks.find(header.kid.as_deref()).await?;
    // The header must not choose how the key is used: the key's own type
    // (and `alg`, `use`, curve) has to fit the algorithm the header names.
    if !key.fits(alg) {
        return Err(refuse("key does not fit the algorithm"));
    }
    let decoding = key
        .decoding_key()
        .ok_or_else(|| refuse("key does not fit the algorithm"))?;

    let mut validation = jsonwebtoken::Validation::new(alg);
    validation.algorithms = vec![alg];
    // The signature only; the claims are checked below against `now`.
    validation.validate_exp = false;
    validation.validate_nbf = false;
    validation.validate_aud = false;
    validation.required_spec_claims.clear();
    let data = jsonwebtoken::decode::<Value>(token, &decoding, &validation).map_err(|e| match e
        .kind()
    {
        jsonwebtoken::errors::ErrorKind::InvalidSignature => refuse("signature invalid"),
        _ => refuse("malformed token"),
    })?;
    let Value::Object(raw) = data.claims else {
        return Err(refuse("malformed token"));
    };

    if raw.get("iss").and_then(Value::as_str) != Some(expected.issuer) {
        return Err(refuse("issuer mismatch"));
    }
    let audiences: Vec<&str> = match raw.get("aud") {
        Some(Value::String(s)) => vec![s.as_str()],
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if !audiences.contains(&expected.client_id) {
        return Err(refuse("audience mismatch"));
    }
    let azp = raw.get("azp");
    let azp_ok = match azp {
        Some(Value::String(s)) => s == expected.client_id,
        Some(_) => false,
        None => audiences.len() == 1,
    };
    if !azp_ok {
        return Err(refuse("authorized party mismatch"));
    }
    let now = expected.now as f64;
    let number = |name: &str| raw.get(name).and_then(Value::as_f64);
    match number("exp") {
        Some(exp) if exp + LEEWAY_SECS > now => {}
        _ => return Err(refuse("token expired")),
    }
    match number("iat") {
        Some(iat) if iat <= now + IAT_AHEAD_SECS + LEEWAY_SECS => {}
        _ => return Err(refuse("token issued in the future")),
    }
    if let Some(nbf) = raw.get("nbf") {
        match nbf.as_f64() {
            Some(nbf) if nbf <= now + LEEWAY_SECS => {}
            _ => return Err(refuse("token not yet valid")),
        }
    }
    match raw.get("nonce").and_then(Value::as_str) {
        Some(nonce) if secrets_equal(nonce, expected.nonce) => {}
        _ => return Err(refuse("nonce mismatch")),
    }
    let subject = raw
        .get("sub")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= MAX_SUBJECT_BYTES)
        .ok_or_else(|| refuse("subject missing"))?
        .to_string();
    Ok(Claims { subject, raw })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::oidc::testkit::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ISS: &str = "https://idp.example.com";
    const CLIENT: &str = "client-1";
    const NONCE: &str = "nonce-1";
    const NOW: i64 = 32_503_680_000; // 3000-01-01

    fn good() -> Value {
        json!({
            "iss": ISS, "aud": CLIENT, "sub": "user-1", "nonce": NONCE,
            "iat": NOW - 10, "exp": NOW + 600, "email": "a@example.com",
        })
    }

    fn expected() -> Expected<'static> {
        Expected {
            issuer: ISS,
            client_id: CLIENT,
            nonce: NONCE,
            now: NOW,
        }
    }

    async fn server_with(keys: Vec<Value>) -> (MockServer, Jwks) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "keys": keys })))
            .mount(&server)
            .await;
        let jwks = Jwks::new(
            crate::app::http_client(),
            reqwest::Url::parse(&format!("{}/jwks", server.uri())).unwrap(),
        );
        (server, jwks)
    }

    fn reason(r: Result<Claims, ExternalError>) -> String {
        match r {
            Err(ExternalError::Token(m)) => m,
            other => panic!("expected a token refusal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn every_allowed_algorithm_is_accepted() {
        let k = test_keys();
        let (_s, jwks) =
            server_with(vec![k.rsa_jwk("rsa"), k.ec_jwk("ec"), k.ec384_jwk("ec384")]).await;
        for alg in ALLOWED {
            let kid = match alg {
                Algorithm::ES256 => "ec",
                Algorithm::ES384 => "ec384",
                _ => "rsa",
            };
            let token = k.sign(alg, Some(kid), &good());
            let claims = validate(&token, &jwks, &expected())
                .await
                .unwrap_or_else(|e| panic!("{alg:?}: {e:?}"));
            assert_eq!(claims.subject, "user-1");
            assert_eq!(claims.text("email").as_deref(), Some("a@example.com"));
        }
    }

    /// Review Focus 1: each check rejects its bad case.
    #[tokio::test]
    async fn every_bad_token_is_refused_for_its_own_reason() {
        let k = test_keys();
        let (_s, jwks) = server_with(vec![k.rsa_jwk("rsa"), k.ec_jwk("ec")]).await;
        let with = |f: &dyn Fn(&mut Value)| {
            let mut c = good();
            f(&mut c);
            k.sign(Algorithm::RS256, Some("rsa"), &c)
        };
        let cases: Vec<(&str, String, &str)> = vec![
            (
                "wrong signature",
                k.sign_with_other_key(Algorithm::RS256, "rsa", &good()),
                "signature",
            ),
            (
                "alg none",
                k.unsigned_none(Some("rsa"), &good()),
                "malformed",
            ),
            (
                "HS256 with the public key as secret",
                k.hs256_with_public_key("rsa", &good()),
                "algorithm",
            ),
            (
                "HS256 with any secret",
                k.sign_hs256(b"secret", "rsa", &good()),
                "algorithm",
            ),
            ("EdDSA", k.fake_alg("EdDSA", "rsa", &good()), "algorithm"),
            (
                "RS256 header over an EC key",
                k.sign(Algorithm::RS256, Some("ec"), &good()),
                "key",
            ),
            (
                "ES256 header over an RSA key",
                k.sign(Algorithm::ES256, Some("rsa"), &good()),
                "key",
            ),
            (
                "wrong iss",
                with(&|c| c["iss"] = json!("https://other.example.com")),
                "issuer",
            ),
            (
                "iss with trailing slash",
                with(&|c| c["iss"] = json!("https://idp.example.com/")),
                "issuer",
            ),
            (
                "missing iss",
                with(&|c| {
                    c.as_object_mut().unwrap().remove("iss");
                }),
                "issuer",
            ),
            (
                "wrong aud",
                with(&|c| c["aud"] = json!("someone-else")),
                "audience",
            ),
            (
                "aud array without us",
                with(&|c| c["aud"] = json!(["a", "b"])),
                "audience",
            ),
            (
                "missing aud",
                with(&|c| {
                    c.as_object_mut().unwrap().remove("aud");
                }),
                "audience",
            ),
            (
                "multi aud without azp",
                with(&|c| c["aud"] = json!([CLIENT, "other"])),
                "authorized",
            ),
            (
                "multi aud with wrong azp",
                with(&|c| {
                    c["aud"] = json!([CLIENT, "other"]);
                    c["azp"] = json!("other");
                }),
                "authorized",
            ),
            (
                "single aud with foreign azp",
                with(&|c| c["azp"] = json!("other")),
                "authorized",
            ),
            ("expired", with(&|c| c["exp"] = json!(NOW - 61)), "expired"),
            (
                "missing exp",
                with(&|c| {
                    c.as_object_mut().unwrap().remove("exp");
                }),
                "expired",
            ),
            (
                "exp as text",
                with(&|c| c["exp"] = json!("9999999999")),
                "expired",
            ),
            (
                "iat far in the future",
                with(&|c| c["iat"] = json!(NOW + 361)),
                "future",
            ),
            (
                "missing iat",
                with(&|c| {
                    c.as_object_mut().unwrap().remove("iat");
                }),
                "future",
            ),
            (
                "nbf in the future",
                with(&|c| c["nbf"] = json!(NOW + 61)),
                "not yet",
            ),
            (
                "wrong nonce",
                with(&|c| c["nonce"] = json!("replayed")),
                "nonce",
            ),
            (
                "missing nonce",
                with(&|c| {
                    c.as_object_mut().unwrap().remove("nonce");
                }),
                "nonce",
            ),
            (
                "missing sub",
                with(&|c| {
                    c.as_object_mut().unwrap().remove("sub");
                }),
                "subject",
            ),
            ("empty sub", with(&|c| c["sub"] = json!("")), "subject"),
            (
                "unknown kid",
                k.sign(Algorithm::RS256, Some("ghost"), &good()),
                "key",
            ),
            ("not a token", "abc.def".to_string(), "malformed"),
            ("empty", String::new(), "malformed"),
            ("huge", "a".repeat(100_000), "malformed"),
        ];
        for (name, token, expect) in cases {
            let r = reason(validate(&token, &jwks, &expected()).await);
            assert!(
                r.contains(expect),
                "{name}: got {r:?}, wanted it to mention {expect:?}"
            );
        }
    }

    #[tokio::test]
    async fn clock_leeway_is_sixty_seconds_each_way() {
        let k = test_keys();
        let (_s, jwks) = server_with(vec![k.rsa_jwk("rsa")]).await;
        let sign = |c: Value| k.sign(Algorithm::RS256, Some("rsa"), &c);
        let mut c = good();
        c["exp"] = json!(NOW - 59);
        assert!(validate(&sign(c), &jwks, &expected()).await.is_ok());
        let mut c = good();
        c["iat"] = json!(NOW + 300);
        assert!(validate(&sign(c), &jwks, &expected()).await.is_ok());
        let mut c = good();
        c["iat"] = json!(NOW + 359);
        assert!(validate(&sign(c), &jwks, &expected()).await.is_ok());
    }

    #[tokio::test]
    async fn aud_may_be_an_array_with_a_matching_azp() {
        let k = test_keys();
        let (_s, jwks) = server_with(vec![k.rsa_jwk("rsa")]).await;
        let mut c = good();
        c["aud"] = json!(["other", CLIENT]);
        c["azp"] = json!(CLIENT);
        assert!(validate(
            &k.sign(Algorithm::RS256, Some("rsa"), &c),
            &jwks,
            &expected()
        )
        .await
        .is_ok());
        let mut c = good();
        c["aud"] = json!([CLIENT]);
        assert!(validate(
            &k.sign(Algorithm::RS256, Some("rsa"), &c),
            &jwks,
            &expected()
        )
        .await
        .is_ok());
    }

    #[tokio::test]
    async fn a_token_without_kid_uses_the_only_key() {
        let k = test_keys();
        let (_s, jwks) = server_with(vec![k.rsa_jwk("rsa")]).await;
        assert!(
            validate(&k.sign(Algorithm::RS256, None, &good()), &jwks, &expected())
                .await
                .is_ok()
        );
        let (_s2, two) = server_with(vec![k.rsa_jwk("a"), k.rsa_jwk("b")]).await;
        assert!(
            validate(&k.sign(Algorithm::RS256, None, &good()), &two, &expected())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_key_only_in_the_refetched_set_is_found() {
        let k = test_keys();
        let (_s, mut jwks) = server_with(vec![k.rsa_jwk("rsa")]).await;
        jwks.min_refetch = std::time::Duration::ZERO;
        // Same server, so the key is there on the refetch only if the kid exists.
        assert!(validate(
            &k.sign(Algorithm::RS256, Some("rsa"), &good()),
            &jwks,
            &expected()
        )
        .await
        .is_ok());
        assert!(reason(
            validate(
                &k.sign(Algorithm::RS256, Some("late"), &good()),
                &jwks,
                &expected()
            )
            .await
        )
        .contains("key"));
    }

    #[test]
    fn claims_read_flags_and_lists_in_every_form() {
        let c = Claims {
            subject: "s".into(),
            raw: json!({
                "a": true, "b": "true", "c": "false", "d": "maybe", "e": 1,
                "g1": ["x", 1, "y"], "g2": "solo", "g3": 5, "g4": [], "name": "  Ann  ", "empty": "  ",
            })
            .as_object()
            .unwrap()
            .clone(),
        };
        assert_eq!(c.flag("a"), Some(true));
        assert_eq!(c.flag("b"), Some(true));
        assert_eq!(c.flag("c"), Some(false));
        assert_eq!(c.flag("d"), None);
        assert_eq!(c.flag("e"), None);
        assert_eq!(c.flag("missing"), None);
        let strings = |name: &str| c.strings_if_present(name);
        assert_eq!(strings("g1"), Some(vec!["x".to_string(), "y".to_string()]));
        assert_eq!(strings("g2"), Some(vec!["solo".to_string()]));
        // A claim of another kind, or none, is unknown; an empty list is not.
        assert_eq!(strings("g3"), None);
        assert_eq!(strings("g4"), Some(vec![]));
        assert_eq!(strings("missing"), None);
        assert!(!format!("{c:?}").contains("Ann"));
        assert_eq!(c.text("name").as_deref(), Some("Ann"));
        assert_eq!(c.text("empty"), None);
        assert_eq!(c.text("e"), None);
    }
}
