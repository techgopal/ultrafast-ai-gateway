//! The in-memory limiter on its own: windows, concurrency, strictest wins.
//! Time is passed in, so nothing here sleeps.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ultrafast_gateway::limits::{LimitScope, Limiter, MemoryLimiter, RateLimit, Subject, Subjects};

fn subject(
    scope: LimitScope,
    id: i64,
    label: &str,
    requests: Option<u64>,
    tokens: Option<u64>,
    concurrent: Option<u64>,
) -> Arc<Subject> {
    Arc::new(Subject {
        scope,
        id,
        label: label.to_string(),
        limit: RateLimit {
            requests_per_minute: requests,
            tokens_per_minute: tokens,
            concurrent,
        },
    })
}

fn key(requests: Option<u64>, tokens: Option<u64>, concurrent: Option<u64>) -> Subjects {
    Subjects {
        key: Some(subject(
            LimitScope::Key,
            1,
            "key 'ci'",
            requests,
            tokens,
            concurrent,
        )),
        ..Subjects::default()
    }
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn nobody_limited_is_always_let_through() {
    let limiter = MemoryLimiter::new();
    let now = Instant::now();
    for _ in 0..1000 {
        limiter
            .acquire(&Subjects::default(), 1_000_000, now)
            .unwrap();
    }
}

#[test]
fn requests_per_minute_slide() {
    let limiter = MemoryLimiter::new();
    let t0 = Instant::now();
    let who = key(Some(2), None, None);
    limiter.acquire(&who, 0, t0).unwrap();
    limiter.acquire(&who, 0, t0 + secs(30)).unwrap();
    let refused = limiter.acquire(&who, 0, t0 + secs(31)).err().unwrap();
    assert_eq!(refused.limit_name, "requests per minute");
    assert_eq!(refused.scope_label, "key 'ci'");
    // The call at t0 leaves the window at t0 + 60.
    assert_eq!(refused.retry_after, secs(29));
    assert!(limiter.acquire(&who, 0, t0 + secs(59)).is_err());
    // One slot is free again; the call at t0 + 30 still counts.
    limiter.acquire(&who, 0, t0 + secs(60)).unwrap();
    assert!(limiter.acquire(&who, 0, t0 + secs(61)).is_err());
    limiter.acquire(&who, 0, t0 + secs(90)).unwrap();
}

#[test]
fn tokens_per_minute_count_estimates_and_settle_corrects() {
    let limiter = MemoryLimiter::new();
    let t0 = Instant::now();
    let who = key(None, Some(100), None);
    let mut first = limiter.acquire(&who, 60, t0).unwrap();
    let refused = limiter.acquire(&who, 60, t0 + secs(1)).err().unwrap();
    assert_eq!(refused.limit_name, "tokens per minute");
    assert_eq!(refused.retry_after, secs(59));
    // The call used 10 tokens, not 60.
    first.settle(10);
    limiter.acquire(&who, 60, t0 + secs(2)).unwrap();
    // And a call may use more than it was charged.
    let mut big = limiter.acquire(&who, 1, t0 + secs(3)).unwrap();
    big.settle(500);
    assert!(limiter.acquire(&who, 1, t0 + secs(4)).is_err());
    // A second settle changes nothing.
    big.settle(0);
    assert!(limiter.acquire(&who, 1, t0 + secs(5)).is_err());
}

#[test]
fn settling_after_the_window_moved_on_does_not_go_negative() {
    let limiter = MemoryLimiter::new();
    let t0 = Instant::now();
    let who = key(None, Some(100), None);
    let mut permit = limiter.acquire(&who, 100, t0).unwrap();
    // The charge has left the window; settling must not take tokens off
    // later calls.
    limiter.acquire(&who, 100, t0 + secs(70)).unwrap();
    permit.settle(0);
    assert!(limiter.acquire(&who, 1, t0 + secs(71)).is_err());
}

#[test]
fn an_estimate_above_the_limit_is_let_through_on_an_empty_window() {
    let limiter = MemoryLimiter::new();
    let t0 = Instant::now();
    let who = key(None, Some(100), None);
    let permit = limiter.acquire(&who, 5_000, t0).unwrap();
    assert!(limiter.acquire(&who, 1, t0 + secs(1)).is_err());
    drop(permit);
}

#[test]
fn concurrency_is_released_on_drop() {
    let limiter = MemoryLimiter::new();
    let t0 = Instant::now();
    let who = key(None, None, Some(2));
    let a = limiter.acquire(&who, 0, t0).unwrap();
    let b = limiter.acquire(&who, 0, t0).unwrap();
    let refused = limiter.acquire(&who, 0, t0).err().unwrap();
    assert_eq!(refused.limit_name, "concurrent requests");
    assert_eq!(refused.retry_after, secs(1));
    drop(a);
    let c = limiter.acquire(&who, 0, t0).unwrap();
    assert!(limiter.acquire(&who, 0, t0).is_err());
    drop(b);
    drop(c);
    limiter.acquire(&who, 0, t0).unwrap();
}

#[test]
fn a_settled_permit_still_holds_its_slot_until_dropped() {
    let limiter = MemoryLimiter::new();
    let t0 = Instant::now();
    let who = key(None, Some(1000), Some(1));
    let mut permit = limiter.acquire(&who, 10, t0).unwrap();
    permit.settle(5);
    assert!(limiter.acquire(&who, 10, t0).is_err());
    drop(permit);
    limiter.acquire(&who, 10, t0).unwrap();
}

#[test]
fn every_scope_is_checked_and_the_message_names_the_one_that_refused() {
    let limiter = MemoryLimiter::new();
    let t0 = Instant::now();
    let make =
        |gateway: Option<u64>, team: Option<u64>, user: Option<u64>, key: Option<u64>| Subjects {
            key: Some(subject(LimitScope::Key, 1, "key 'ci'", key, None, None)),
            user: Some(subject(
                LimitScope::User,
                2,
                "user 'lena@example.com'",
                user,
                None,
                None,
            )),
            teams: vec![subject(
                LimitScope::Team,
                3,
                "team 'Platform'",
                team,
                None,
                None,
            )],
            gateway: Some(subject(
                LimitScope::Gateway,
                0,
                "gateway",
                gateway,
                None,
                None,
            )),
        };
    for (who, label) in [
        (make(Some(1), None, None, None), "gateway"),
        (make(None, Some(1), None, None), "team 'Platform'"),
        (make(None, None, Some(1), None), "user 'lena@example.com'"),
        (make(None, None, None, Some(1)), "key 'ci'"),
    ] {
        // A limiter of its own: scopes do not share counters across cases.
        let limiter = MemoryLimiter::new();
        limiter.acquire(&who, 0, t0).unwrap();
        let refused = limiter.acquire(&who, 0, t0).err().unwrap();
        assert_eq!(refused.scope_label, label);
        assert_eq!(
            refused.message(),
            format!("rate limit 'requests per minute' of {label} reached")
        );
    }
    drop(limiter);
}

#[test]
fn the_strictest_limit_applies() {
    let limiter = MemoryLimiter::new();
    let t0 = Instant::now();
    let who = Subjects {
        key: Some(subject(LimitScope::Key, 1, "key 'ci'", Some(5), None, None)),
        user: Some(subject(
            LimitScope::User,
            2,
            "user 'a'",
            Some(3),
            None,
            None,
        )),
        teams: vec![
            subject(LimitScope::Team, 3, "team 'Platform'", Some(2), None, None),
            subject(LimitScope::Team, 4, "team 'Research'", Some(10), None, None),
        ],
        gateway: Some(subject(
            LimitScope::Gateway,
            0,
            "gateway",
            Some(4),
            None,
            None,
        )),
    };
    limiter.acquire(&who, 0, t0).unwrap();
    limiter.acquire(&who, 0, t0).unwrap();
    let refused = limiter.acquire(&who, 0, t0).err().unwrap();
    assert_eq!(refused.scope_label, "team 'Platform'");
}

#[test]
fn when_several_refuse_the_longest_wait_is_reported() {
    let limiter = MemoryLimiter::new();
    let t0 = Instant::now();
    let who = Subjects {
        key: Some(subject(LimitScope::Key, 1, "key 'ci'", Some(1), None, None)),
        gateway: Some(subject(
            LimitScope::Gateway,
            0,
            "gateway",
            Some(2),
            None,
            None,
        )),
        ..Subjects::default()
    };
    // The gateway's window holds a call from t0, the key's from t0 + 30.
    let gateway_only = Subjects {
        gateway: who.gateway.clone(),
        ..Subjects::default()
    };
    limiter.acquire(&gateway_only, 0, t0).unwrap();
    limiter.acquire(&who, 0, t0 + secs(30)).unwrap();
    let refused = limiter.acquire(&who, 0, t0 + secs(40)).err().unwrap();
    // Key: the call of t0 + 30 leaves at t0 + 90 (50 s). Gateway: the call of
    // t0 leaves at t0 + 60 (20 s). The wait that matters is the longer one.
    assert_eq!(refused.scope_label, "key 'ci'");
    assert_eq!(refused.retry_after, secs(50));
}

#[test]
fn a_refused_call_adds_nothing_anywhere() {
    let limiter = MemoryLimiter::new();
    let t0 = Instant::now();
    let key_subject = subject(LimitScope::Key, 1, "key 'ci'", Some(2), Some(1000), Some(2));
    let team_subject = subject(LimitScope::Team, 3, "team 'Platform'", Some(1), None, None);
    let both = Subjects {
        key: Some(key_subject.clone()),
        teams: vec![team_subject],
        ..Subjects::default()
    };
    let key_only = Subjects {
        key: Some(key_subject),
        ..Subjects::default()
    };
    let first = limiter.acquire(&both, 100, t0).unwrap();
    // Refused by the team, again and again.
    for i in 1..=5 {
        assert!(limiter.acquire(&both, 100, t0 + secs(i)).is_err());
    }
    drop(first);
    // The key has had one request, not six, and no refused slot is held.
    let second = limiter.acquire(&key_only, 100, t0 + secs(10)).unwrap();
    let third = limiter.acquire(&key_only, 100, t0 + secs(11));
    assert!(third.is_err(), "the key's own limit of 2 is now reached");
    drop(second);
}

#[test]
fn parallel_acquires_never_exceed_the_concurrency_limit() {
    let limiter = Arc::new(MemoryLimiter::new());
    let who = Arc::new(key(None, None, Some(2)));
    let t0 = Instant::now();
    let barrier = Arc::new(std::sync::Barrier::new(20));
    let handles: Vec<_> = (0..20)
        .map(|_| {
            let (limiter, who, barrier) = (limiter.clone(), who.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                limiter.acquire(&who, 0, t0).ok()
            })
        })
        .collect();
    let permits: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(permits.iter().filter(|p| p.is_some()).count(), 2);
}

#[test]
fn limits_changed_between_calls_apply_at_once_and_counters_stay() {
    let limiter = MemoryLimiter::new();
    let t0 = Instant::now();
    limiter.acquire(&key(Some(1), None, None), 0, t0).unwrap();
    // The limit is raised (a new snapshot): the call already counted still counts.
    limiter.acquire(&key(Some(2), None, None), 0, t0).unwrap();
    assert!(limiter.acquire(&key(Some(2), None, None), 0, t0).is_err());
    // A permit taken under a concurrency limit that was removed meanwhile
    // still releases cleanly.
    let permit = limiter.acquire(&key(None, None, Some(1)), 0, t0).unwrap();
    limiter.acquire(&key(None, None, None), 0, t0).unwrap();
    drop(permit);
    limiter.acquire(&key(None, None, Some(1)), 0, t0).unwrap();
}
