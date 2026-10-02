//! The key of a cached answer: a SHA-256 over a canonical encoding of
//! everything that can change the answer, and of whom the answer is for.

use sha2::{Digest, Sha256};
use ultrafast_translate::embeddings::EmbeddingsRequest;
use ultrafast_translate::types::{ChatRequest, Message, Role};

use super::ScopeId;

/// The 32 bytes a cached answer is found by.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CacheKey([u8; 32]);

impl std::fmt::Debug for CacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CacheKey({})", hex::encode(&self.0[..6]))
    }
}

/// What a call is cached under besides its own fields.
#[derive(Debug, Clone, Copy)]
pub struct KeyParts<'a> {
    /// The name of the route.
    pub route: &'a str,
    /// `(provider, model)` of every target of the route the caller may
    /// call, sorted.
    pub targets: &'a [(String, String)],
    pub scope: ScopeId,
}

/// Fields are tagged and variable parts are length-prefixed, so no two
/// different calls encode to the same bytes.
struct Encoder(Sha256);

impl Encoder {
    fn new(kind: &str) -> Self {
        let mut e = Self(Sha256::new());
        e.field(0, kind.as_bytes());
        e
    }

    fn field(&mut self, tag: u8, bytes: &[u8]) {
        self.0.update([tag]);
        self.0.update((bytes.len() as u64).to_le_bytes());
        self.0.update(bytes);
    }

    fn number(&mut self, tag: u8, n: u64) {
        self.field(tag, &n.to_le_bytes());
    }

    /// A field that may be absent: absent and empty differ.
    fn optional(&mut self, tag: u8, bytes: Option<&[u8]>) {
        match bytes {
            Some(b) => {
                self.0.update([tag, 1]);
                self.0.update((b.len() as u64).to_le_bytes());
                self.0.update(b);
            }
            None => self.0.update([tag, 0]),
        }
    }

    /// Negative zero is zero.
    fn float(&mut self, tag: u8, v: Option<f32>) {
        let bits = v.map(|v| if v == 0.0 { 0 } else { v.to_bits() });
        self.optional(tag, bits.map(u32::to_le_bytes).as_ref().map(|b| &b[..]));
    }

    fn parts(&mut self, parts: &KeyParts<'_>) {
        self.field(1, parts.route.as_bytes());
        self.number(2, parts.targets.len() as u64);
        for (provider, model) in parts.targets {
            self.field(3, provider.as_bytes());
            self.field(4, model.as_bytes());
        }
        self.field(5, parts.scope.scope.as_str().as_bytes());
        self.number(6, parts.scope.id as u64);
    }

    fn finish(self) -> CacheKey {
        CacheKey(self.0.finalize().into())
    }
}

fn role(r: Role) -> &'static str {
    match r {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

impl CacheKey {
    /// A chat call, whichever shape it came in. Every field of the request
    /// is named here, so a field added to it does not compile until it is
    /// either part of the key or left out on purpose.
    pub fn chat(parts: &KeyParts<'_>, request: &ChatRequest) -> Self {
        let ChatRequest {
            model,
            messages,
            max_tokens,
            temperature,
            top_p,
            stop,
            stream,
        } = request;
        let mut e = Encoder::new("chat");
        e.parts(parts);
        e.field(10, model.as_bytes());
        e.number(11, messages.len() as u64);
        for Message {
            role: r,
            content,
            name,
        } in messages
        {
            e.field(12, role(*r).as_bytes());
            e.field(13, content.as_bytes());
            e.optional(14, name.as_deref().map(str::as_bytes));
        }
        e.optional(
            15,
            max_tokens.map(u32::to_le_bytes).as_ref().map(|b| &b[..]),
        );
        e.float(16, *temperature);
        e.float(17, *top_p);
        match stop {
            Some(stop) => {
                e.number(18, stop.len() as u64);
                for s in stop {
                    e.field(19, s.as_bytes());
                }
            }
            None => e.optional(18, None),
        }
        e.number(20, u64::from(*stream));
        e.finish()
    }

    pub fn embeddings(parts: &KeyParts<'_>, request: &EmbeddingsRequest) -> Self {
        let EmbeddingsRequest {
            model,
            input,
            dimensions,
        } = request;
        let mut e = Encoder::new("embeddings");
        e.parts(parts);
        e.field(10, model.as_bytes());
        e.number(11, input.len() as u64);
        for s in input {
            e.field(12, s.as_bytes());
        }
        e.optional(
            13,
            dimensions.map(u32::to_le_bytes).as_ref().map(|b| &b[..]),
        );
        e.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheScope;

    type Change = Box<dyn Fn(&mut ChatRequest)>;

    fn targets() -> Vec<(String, String)> {
        vec![("p".into(), "gpt-4o".into()), ("p".into(), "m".into())]
    }

    fn scope(scope: CacheScope, id: i64) -> ScopeId {
        ScopeId { scope, id }
    }

    fn base() -> ChatRequest {
        ChatRequest {
            model: "r".into(),
            messages: vec![
                Message {
                    role: Role::System,
                    content: "be brief".into(),
                    name: None,
                },
                Message {
                    role: Role::User,
                    content: "hi".into(),
                    name: Some("lena".into()),
                },
            ],
            max_tokens: Some(10),
            temperature: Some(0.2),
            top_p: Some(0.9),
            stop: Some(vec!["END".into()]),
            stream: false,
        }
    }

    fn key_of(request: &ChatRequest) -> CacheKey {
        let t = targets();
        CacheKey::chat(
            &KeyParts {
                route: "r",
                targets: &t,
                scope: scope(CacheScope::Team, 1),
            },
            request,
        )
    }

    #[test]
    fn the_same_call_has_the_same_key() {
        assert_eq!(key_of(&base()), key_of(&base()));
    }

    #[test]
    fn changing_any_one_field_changes_the_key() {
        let original = key_of(&base());
        let changes: Vec<(&str, Change)> = vec![
            ("model", Box::new(|r| r.model = "r2".into())),
            (
                "message content",
                Box::new(|r| r.messages[1].content = "ho".into()),
            ),
            (
                "message role",
                Box::new(|r| r.messages[1].role = Role::Assistant),
            ),
            (
                "message name",
                Box::new(|r| r.messages[1].name = Some("tomas".into())),
            ),
            (
                "message name removed",
                Box::new(|r| r.messages[1].name = None),
            ),
            (
                "system message",
                Box::new(|r| r.messages[0].content = "be long".into()),
            ),
            (
                "a message added",
                Box::new(|r| r.messages.push(r.messages[1].clone())),
            ),
            ("message order", Box::new(|r| r.messages.reverse())),
            ("max_tokens", Box::new(|r| r.max_tokens = Some(11))),
            ("max_tokens removed", Box::new(|r| r.max_tokens = None)),
            ("temperature", Box::new(|r| r.temperature = Some(0.3))),
            ("temperature removed", Box::new(|r| r.temperature = None)),
            ("top_p", Box::new(|r| r.top_p = Some(0.8))),
            ("top_p removed", Box::new(|r| r.top_p = None)),
            ("stop", Box::new(|r| r.stop = Some(vec!["STOP".into()]))),
            (
                "stop added",
                Box::new(|r| r.stop.as_mut().unwrap().push("X".into())),
            ),
            ("stop removed", Box::new(|r| r.stop = None)),
            ("stop empty", Box::new(|r| r.stop = Some(vec![]))),
            ("stream", Box::new(|r| r.stream = true)),
        ];
        let mut seen = vec![original];
        for (field, change) in changes {
            let mut r = base();
            change(&mut r);
            let key = key_of(&r);
            assert!(!seen.contains(&key), "{field} does not change the key");
            seen.push(key);
        }
    }

    #[test]
    fn what_is_around_the_call_changes_the_key() {
        let request = base();
        let t = targets();
        let key = |route: &str, targets: &[(String, String)], scope: ScopeId| {
            CacheKey::chat(
                &KeyParts {
                    route,
                    targets,
                    scope,
                },
                &request,
            )
        };
        let team1 = scope(CacheScope::Team, 1);
        let original = key("r", &t, team1);
        assert_ne!(original, key("r2", &t, team1), "route");
        assert_ne!(original, key("r", &t[..1], team1), "a target fewer");
        let other = [("q".to_string(), "m".to_string()), t[1].clone()];
        assert_ne!(original, key("r", &other, team1), "another provider");
        assert_ne!(original, key("r", &t, scope(CacheScope::Team, 2)), "team");
        assert_ne!(
            original,
            key("r", &t, scope(CacheScope::User, 1)),
            "scope kind"
        );
        assert_ne!(
            original,
            key("r", &t, scope(CacheScope::Key, 1)),
            "scope kind"
        );
    }

    #[test]
    fn neighbouring_fields_do_not_blur() {
        // "ab" + "c" is not "a" + "bc".
        let mut a = base();
        a.messages[0].content = "ab".into();
        a.messages[1].content = "c".into();
        let mut b = base();
        b.messages[0].content = "a".into();
        b.messages[1].content = "bc".into();
        assert_ne!(key_of(&a), key_of(&b));
        // Content moved into the name.
        let mut c = base();
        c.messages[1].name = None;
        c.messages[1].content = "hilena".into();
        let mut d = base();
        d.messages[1].name = Some("lena".into());
        d.messages[1].content = "hi".into();
        assert_ne!(key_of(&c), key_of(&d));
        // An absent name is not an empty one.
        let mut e = base();
        e.messages[1].name = Some(String::new());
        let mut f = base();
        f.messages[1].name = None;
        assert_ne!(key_of(&e), key_of(&f));
    }

    #[test]
    fn negative_zero_is_zero() {
        let mut a = base();
        a.temperature = Some(0.0);
        let mut b = base();
        b.temperature = Some(-0.0);
        assert_eq!(key_of(&a), key_of(&b));
    }

    #[test]
    fn embeddings_have_their_own_keys() {
        let t = targets();
        let parts = KeyParts {
            route: "r",
            targets: &t,
            scope: scope(CacheScope::Team, 1),
        };
        let request = EmbeddingsRequest {
            model: "r".into(),
            input: vec!["a".into(), "b".into()],
            dimensions: Some(8),
        };
        let key = |change: &dyn Fn(&mut EmbeddingsRequest)| {
            let mut r = request.clone();
            change(&mut r);
            CacheKey::embeddings(&parts, &r)
        };
        let original = key(&|_| {});
        assert_eq!(original, key(&|_| {}));
        assert_ne!(original, key(&|r| r.model = "r2".into()));
        assert_ne!(original, key(&|r| r.input[1] = "c".into()));
        assert_ne!(original, key(&|r| r.input.reverse()));
        assert_ne!(original, key(&|r| r.input = vec!["ab".into()]));
        assert_ne!(original, key(&|r| r.dimensions = Some(9)));
        assert_ne!(original, key(&|r| r.dimensions = None));
        // Never the key of a chat call.
        let chat = CacheKey::chat(&parts, &base());
        assert_ne!(original, chat);
    }
}
