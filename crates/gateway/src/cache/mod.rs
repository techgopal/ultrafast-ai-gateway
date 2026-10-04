//! The exact-match response cache of a route.
//!
//! A route with the cache on keeps the answer to a call that is not a stream
//! and not random (temperature at most 0.5), and answers the same call again
//! without any provider. "The same call" is [`CacheKey`]: every field that
//! can change the answer, the route and its callable targets, and whom the
//! answer is for ([`CacheScope`]): a team's answer is never another team's.
//!
//! The entries live behind [`ResponseCache`] so a shared store can replace
//! the in-memory one.

mod key;

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

pub use key::{CacheKey, KeyParts};
use ultrafast_translate::embeddings::EmbeddingsResponse;
use ultrafast_translate::types::{ChatResponse, Usage};

/// The most entries the in-memory cache holds.
pub const MAX_ENTRIES: usize = 10_000;
/// The most bytes of answers the in-memory cache holds.
pub const MAX_BYTES: usize = 64 * 1024 * 1024;

/// The longest and shortest time an answer may be kept, in seconds.
pub const TTL_RANGE: std::ops::RangeInclusive<i64> = 1..=86_400;
/// How long an answer is kept when a route does not say.
pub const DEFAULT_TTL_S: i64 = 300;

/// Whom a cached answer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheScope {
    /// Callers whose key belongs to the same team.
    Team,
    /// Callers of the same key.
    Key,
    /// Callers whose key belongs to the same user.
    User,
}

impl CacheScope {
    pub fn as_str(self) -> &'static str {
        match self {
            CacheScope::Team => "team",
            CacheScope::Key => "key",
            CacheScope::User => "user",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "team" => Some(CacheScope::Team),
            "key" => Some(CacheScope::Key),
            "user" => Some(CacheScope::User),
            _ => None,
        }
    }
}

/// The cache settings of a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteCache {
    pub enabled: bool,
    pub ttl_s: i64,
    pub scope: CacheScope,
}

impl Default for RouteCache {
    fn default() -> Self {
        Self {
            enabled: false,
            ttl_s: DEFAULT_TTL_S,
            scope: CacheScope::Team,
        }
    }
}

/// Whom an answer is kept for, as an id: the team, user or key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeId {
    pub scope: CacheScope,
    pub id: i64,
}

impl ScopeId {
    /// The scope a call is cached under. A key with no team under scope
    /// `team` is cached as under `user`, and a key with no user under scope
    /// `user` (or none of either under `team`) as under `key`: an answer is
    /// never kept for "no one", which every such key would share.
    pub fn of(scope: CacheScope, team_id: Option<i64>, user_id: Option<i64>, key_id: i64) -> Self {
        let team = || team_id.map(|id| (CacheScope::Team, id));
        let user = || user_id.map(|id| (CacheScope::User, id));
        let (scope, id) = match scope {
            CacheScope::Team => team().or_else(user),
            CacheScope::User => user(),
            CacheScope::Key => None,
        }
        .unwrap_or((CacheScope::Key, key_id));
        Self { scope, id }
    }
}

/// The answer that is kept, in the form of no ingress shape: it is rendered
/// as the caller's shape when it is given out.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Chat(ChatResponse),
    Embeddings(EmbeddingsResponse),
}

/// A kept answer, and the target that gave it (for the call's record).
#[derive(Debug, Clone, PartialEq)]
pub struct Cached {
    pub answer: Answer,
    pub provider: String,
    pub model: String,
}

impl Cached {
    /// What the call that was answered by it used, as the provider said.
    pub fn usage(&self) -> Option<Usage> {
        match &self.answer {
            Answer::Chat(r) => r.usage,
            Answer::Embeddings(r) => Some(Usage {
                input_tokens: r.prompt_tokens,
                output_tokens: 0,
            }),
        }
    }

    /// About how many bytes it holds, for the bound of the cache.
    pub fn size(&self) -> usize {
        let answer = match &self.answer {
            Answer::Chat(r) => {
                r.id.len()
                    + r.model.len()
                    + r.content.len()
                    + std::mem::size_of_val(&r.finish_reason)
                    + std::mem::size_of_val(&r.usage)
            }
            Answer::Embeddings(r) => {
                r.model.len() + r.vectors.iter().map(|v| v.len() * 4 + 24).sum::<usize>()
            }
        };
        // The key, the map slot and the order slot are held too.
        let overhead = std::mem::size_of::<CacheKey>() * 2
            + std::mem::size_of::<Entry>()
            + 2 * std::mem::size_of::<u64>() * 4;
        std::mem::size_of::<Self>() + overhead + self.provider.len() + self.model.len() + answer
    }
}

/// Keeps answers. `now` is passed in, so no implementation reads a clock.
pub trait ResponseCache: Send + Sync {
    /// The answer kept under `key`, unless it has expired.
    /// A hit is shared, not copied: the answer is cloned by reference.
    fn get(&self, key: &CacheKey, now: Instant) -> Option<Arc<Cached>>;
    /// Keeps `value` for `ttl`, replacing what the key had.
    fn put(&self, key: CacheKey, value: Cached, ttl: Duration, now: Instant);
    /// Forgets every answer.
    fn clear(&self);
}

struct Entry {
    value: Arc<Cached>,
    expires: Instant,
    bytes: usize,
    /// When it was last used: its place in `order`.
    used: u64,
}

#[derive(Default)]
struct Shared {
    entries: HashMap<CacheKey, Entry>,
    /// Least recently used first.
    order: BTreeMap<u64, CacheKey>,
    bytes: usize,
    tick: u64,
}

impl Shared {
    fn remove(&mut self, key: &CacheKey) {
        if let Some(entry) = self.entries.remove(key) {
            self.order.remove(&entry.used);
            self.bytes -= entry.bytes;
        }
    }
}

/// Answers in the memory of this process, least recently used out first
/// when there are too many or too large. They start empty on every start
/// and are not shared between processes.
pub struct MemoryCache {
    max_entries: usize,
    max_bytes: usize,
    shared: Mutex<Shared>,
}

impl Default for MemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryCache {
    pub fn new() -> Self {
        Self::with_bounds(MAX_ENTRIES, MAX_BYTES)
    }

    pub fn with_bounds(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            max_entries,
            max_bytes,
            shared: Mutex::default(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How many entries are held, expired ones included.
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many bytes the entries hold.
    pub fn bytes(&self) -> usize {
        self.lock().bytes
    }
}

impl ResponseCache for MemoryCache {
    fn clear(&self) {
        *self.lock() = Shared::default();
    }

    fn get(&self, key: &CacheKey, now: Instant) -> Option<Arc<Cached>> {
        let mut shared = self.lock();
        let entry = shared.entries.get(key)?;
        if entry.expires <= now {
            shared.remove(key);
            return None;
        }
        // The Arc is cloned under the lock; the answer is not.
        let value = Arc::clone(&entry.value);
        shared.tick += 1;
        let tick = shared.tick;
        let used = std::mem::replace(&mut shared.entries.get_mut(key)?.used, tick);
        shared.order.remove(&used);
        shared.order.insert(tick, *key);
        Some(value)
    }

    fn put(&self, key: CacheKey, value: Cached, ttl: Duration, now: Instant) {
        let bytes = value.size();
        let mut shared = self.lock();
        shared.remove(&key);
        // An answer larger than the whole cache is not kept; it would
        // push out everything and then itself.
        if bytes > self.max_bytes || self.max_entries == 0 {
            return;
        }
        while shared.entries.len() >= self.max_entries
            || shared.bytes.saturating_add(bytes) > self.max_bytes
        {
            let Some((_, oldest)) = shared.order.pop_first() else {
                break;
            };
            if let Some(gone) = shared.entries.remove(&oldest) {
                shared.bytes -= gone.bytes;
            }
        }
        shared.tick += 1;
        let used = shared.tick;
        shared.order.insert(used, key);
        shared.bytes += bytes;
        shared.entries.insert(
            key,
            Entry {
                value: Arc::new(value),
                expires: now + ttl,
                bytes,
                used,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultrafast_translate::types::{ChatRequest, Message, Role};

    fn answer(content: &str) -> Cached {
        Cached {
            answer: Answer::Chat(ChatResponse {
                id: "c1".into(),
                model: "gpt-4o".into(),
                content: content.into(),
                finish_reason: None,
                usage: None,
            }),
            provider: "p".into(),
            model: "gpt-4o".into(),
        }
    }

    fn key(n: u32) -> CacheKey {
        let request = ChatRequest {
            model: "r".into(),
            messages: vec![Message {
                role: Role::User,
                content: n.to_string(),
                name: None,
            }],
            max_tokens: None,
            temperature: None,
            top_p: None,
            stop: None,
            stream: false,
        };
        CacheKey::chat(
            &KeyParts {
                route: "r",
                targets: &[],
                scope: ScopeId {
                    scope: CacheScope::Team,
                    id: 1,
                },
                config: [0; 32],
            },
            &request,
        )
    }

    const TTL: Duration = Duration::from_secs(60);

    #[test]
    fn a_kept_answer_is_given_back_until_it_expires() {
        let cache = MemoryCache::new();
        let now = Instant::now();
        assert!(cache.get(&key(1), now).is_none());
        cache.put(key(1), answer("a"), TTL, now);
        assert_eq!(cache.get(&key(1), now).as_deref(), Some(&answer("a")));
        assert!(cache.get(&key(1), now + Duration::from_secs(59)).is_some());
        // Not after its time, and the entry is gone.
        assert!(cache.get(&key(1), now + TTL).is_none());
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.bytes(), 0);
    }

    #[test]
    fn a_hit_shares_the_answer_and_does_not_copy_it() {
        let cache = MemoryCache::new();
        let now = Instant::now();
        cache.put(key(1), answer(&"x".repeat(10_000)), TTL, now);
        let a = cache.get(&key(1), now).unwrap();
        let b = cache.get(&key(1), now).unwrap();
        assert!(Arc::ptr_eq(&a, &b), "both hits hold the one stored answer");
    }

    #[test]
    fn putting_again_replaces_the_answer_and_the_time() {
        let cache = MemoryCache::new();
        let now = Instant::now();
        cache.put(key(1), answer("a"), TTL, now);
        let later = now + Duration::from_secs(50);
        cache.put(key(1), answer("bb"), TTL, later);
        assert_eq!(cache.len(), 1);
        assert_eq!(
            cache
                .get(&key(1), now + Duration::from_secs(100))
                .as_deref(),
            Some(&answer("bb"))
        );
        assert_eq!(cache.bytes(), answer("bb").size());
    }

    #[test]
    fn the_least_recently_used_goes_first_when_there_are_too_many() {
        let cache = MemoryCache::with_bounds(3, usize::MAX);
        let now = Instant::now();
        for n in 1..=3 {
            cache.put(key(n), answer("a"), TTL, now);
        }
        // Reading 1 makes 2 the oldest.
        assert!(cache.get(&key(1), now).is_some());
        cache.put(key(4), answer("a"), TTL, now);
        assert_eq!(cache.len(), 3);
        assert!(cache.get(&key(2), now).is_none());
        for n in [1, 3, 4] {
            assert!(cache.get(&key(n), now).is_some(), "{n}");
        }
    }

    #[test]
    fn the_least_recently_used_goes_first_when_there_are_too_many_bytes() {
        let one = answer("x").size();
        // Room for three small answers.
        let cache = MemoryCache::with_bounds(100, one * 3);
        let now = Instant::now();
        for n in 1..=3 {
            cache.put(key(n), answer("x"), TTL, now);
        }
        assert_eq!(cache.bytes(), one * 3);
        assert!(cache.get(&key(1), now).is_some());
        // An answer of three times the size pushes out two.
        let big = answer("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx");
        let big_size = big.size();
        assert!(big_size > one && big_size <= one * 3);
        cache.put(key(9), big, TTL, now);
        assert!(cache.bytes() <= one * 3);
        assert!(cache.get(&key(9), now).is_some());
        // 2 and 3 were the oldest; 1 was read last.
        assert!(cache.get(&key(2), now).is_none());
    }

    #[test]
    fn an_answer_larger_than_the_cache_is_not_kept_and_costs_nothing_else() {
        let cache = MemoryCache::with_bounds(10, answer("x").size());
        let now = Instant::now();
        cache.put(key(1), answer("x"), TTL, now);
        cache.put(key(2), answer(&"y".repeat(100)), TTL, now);
        assert!(cache.get(&key(2), now).is_none());
        // What was kept stays.
        assert!(cache.get(&key(1), now).is_some());
        // Nor does a replacement that is too large keep the old answer.
        cache.put(key(1), answer(&"y".repeat(100)), TTL, now);
        assert!(cache.get(&key(1), now).is_none());
        assert_eq!(cache.bytes(), 0);
    }

    #[test]
    fn the_counts_stay_true_through_replacing_and_evicting() {
        let cache = MemoryCache::with_bounds(2, usize::MAX);
        let now = Instant::now();
        for n in 0..50 {
            cache.put(key(n % 5), answer(&"z".repeat(n as usize)), TTL, now);
            let held: usize = cache.lock().entries.values().map(|e| e.bytes).sum();
            assert_eq!(cache.bytes(), held);
            assert!(cache.len() <= 2);
            let ordered = cache.lock().order.len();
            assert_eq!(ordered, cache.len());
        }
    }

    #[test]
    fn the_scope_of_a_call_falls_back_from_team_to_user_to_key() {
        let of = |scope, team, user| ScopeId::of(scope, team, user, 7);
        let is = |s: ScopeId| (s.scope, s.id);
        assert_eq!(
            is(of(CacheScope::Team, Some(1), Some(2))),
            (CacheScope::Team, 1)
        );
        assert_eq!(
            is(of(CacheScope::Team, None, Some(2))),
            (CacheScope::User, 2)
        );
        assert_eq!(is(of(CacheScope::Team, None, None)), (CacheScope::Key, 7));
        assert_eq!(
            is(of(CacheScope::User, Some(1), Some(2))),
            (CacheScope::User, 2)
        );
        assert_eq!(
            is(of(CacheScope::User, Some(1), None)),
            (CacheScope::Key, 7)
        );
        assert_eq!(
            is(of(CacheScope::Key, Some(1), Some(2))),
            (CacheScope::Key, 7)
        );
    }

    #[test]
    fn scope_names_round_trip() {
        for s in [CacheScope::Team, CacheScope::Key, CacheScope::User] {
            assert_eq!(CacheScope::parse(s.as_str()), Some(s));
        }
        assert_eq!(CacheScope::parse("gateway"), None);
    }
}
