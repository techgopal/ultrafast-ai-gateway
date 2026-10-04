//! A sliding window of the last 60 seconds, in one bucket per second.

use std::time::Duration;

/// How many seconds the window spans.
pub const SPAN: u64 = 60;

#[derive(Debug, Clone, Copy)]
struct Bucket {
    /// The second this bucket counts; `EMPTY` before the first use.
    second: u64,
    requests: u64,
    tokens: u64,
}

const EMPTY: u64 = u64::MAX;

impl Bucket {
    const NONE: Bucket = Bucket {
        second: EMPTY,
        requests: 0,
        tokens: 0,
    };

    /// Whether the bucket still counts at `now`.
    fn live(&self, now: u64) -> bool {
        self.second != EMPTY && self.second <= now && self.second + SPAN > now
    }
}

/// Requests and tokens of the last 60 seconds. Seconds are counted from
/// any fixed origin; the caller passes them in, so the window reads no clock.
#[derive(Debug, Clone)]
pub struct Window {
    buckets: [Bucket; SPAN as usize],
}

impl Default for Window {
    fn default() -> Self {
        Self {
            buckets: [Bucket::NONE; SPAN as usize],
        }
    }
}

fn index(second: u64) -> usize {
    (second % SPAN) as usize
}

impl Window {
    /// Requests and tokens counted in the 60 seconds up to and including `now`.
    pub fn used(&self, now: u64) -> (u64, u64) {
        self.buckets
            .iter()
            .filter(|b| b.live(now))
            .fold((0, 0), |(r, t), b| (r + b.requests, t + b.tokens))
    }

    pub fn add(&mut self, now: u64, requests: u64, tokens: u64) {
        let bucket = &mut self.buckets[index(now)];
        if bucket.second != now {
            *bucket = Bucket {
                second: now,
                requests: 0,
                tokens: 0,
            };
        }
        bucket.requests = bucket.requests.saturating_add(requests);
        bucket.tokens = bucket.tokens.saturating_add(tokens);
    }

    /// Takes back one request counted at `at`, if that second is still in
    /// the window as of `now`.
    pub fn take_request(&mut self, at: u64, now: u64) {
        let bucket = &mut self.buckets[index(at)];
        if bucket.second == at && bucket.live(now) {
            bucket.requests = bucket.requests.saturating_sub(1);
        }
    }

    /// Corrects tokens charged at `charged_at` to what they came to: less
    /// is taken back from that second, if it is still in the window; more
    /// is added to `now`.
    pub fn correct_tokens(&mut self, charged_at: u64, charged: u64, actual: u64, now: u64) {
        if actual > charged {
            self.add(now, 0, actual - charged);
            return;
        }
        let bucket = &mut self.buckets[index(charged_at)];
        if bucket.second == charged_at && bucket.live(now) {
            bucket.tokens = bucket.tokens.saturating_sub(charged - actual);
        }
    }

    /// How long until `count` more requests (`tokens` false) or tokens fit
    /// under `limit`, as the window empties: always a whole number of
    /// seconds, at least one.
    pub fn wait(&self, now: u64, limit: u64, add: u64, tokens: bool) -> Duration {
        let pick = |b: &Bucket| if tokens { b.tokens } else { b.requests };
        let mut used: u64 = self.buckets.iter().filter(|b| b.live(now)).map(pick).sum();
        // The oldest second leaves first.
        let first = now.saturating_sub(SPAN - 1);
        for second in first..=now {
            if used.saturating_add(add) <= limit {
                break;
            }
            let bucket = &self.buckets[index(second)];
            if bucket.second == second {
                used = used.saturating_sub(pick(bucket));
                if used.saturating_add(add) <= limit {
                    return Duration::from_secs((second + SPAN).saturating_sub(now).max(1));
                }
            }
        }
        Duration::from_secs(SPAN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_the_last_sixty_seconds_only() {
        let mut w = Window::default();
        w.add(10, 1, 5);
        w.add(10, 1, 5);
        w.add(40, 1, 1);
        assert_eq!(w.used(40), (3, 11));
        assert_eq!(w.used(69), (3, 11));
        // Second 10 is out at 70.
        assert_eq!(w.used(70), (1, 1));
        assert_eq!(w.used(100), (0, 0));
    }

    #[test]
    fn a_reused_bucket_starts_empty() {
        let mut w = Window::default();
        w.add(5, 3, 3);
        w.add(65, 1, 1);
        assert_eq!(w.used(65), (1, 1));
    }

    #[test]
    fn wait_is_until_enough_has_left() {
        let mut w = Window::default();
        w.add(0, 1, 40);
        w.add(30, 1, 40);
        // Two requests used, limit two: the first one leaves at 60.
        assert_eq!(w.wait(31, 2, 1, false), Duration::from_secs(29));
        // 80 tokens used of 100, 40 wanted: the first 40 leave at 60.
        assert_eq!(w.wait(31, 100, 40, true), Duration::from_secs(29));
        // 70 wanted: both must leave.
        assert_eq!(w.wait(31, 100, 70, true), Duration::from_secs(59));
        // At least a second.
        assert_eq!(w.wait(59, 2, 1, false), Duration::from_secs(1));
    }

    #[test]
    fn correcting_tokens() {
        let mut w = Window::default();
        w.add(10, 1, 100);
        w.correct_tokens(10, 100, 30, 12);
        assert_eq!(w.used(12), (1, 30));
        w.correct_tokens(10, 100, 130, 12);
        assert_eq!(w.used(12), (1, 60));
        // A charge that has left the window is not corrected.
        w.correct_tokens(10, 100, 0, 70);
        assert_eq!(w.used(70), (0, 30));
    }
}
