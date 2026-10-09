//! Running guardrails over a call: the set that applies, and a check over the
//! text slots of a request or an answer.

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use tokio::sync::Semaphore;

use super::external::{self, CallMeta, HookGates};
use super::log::GuardrailRef;
use super::{check_texts, Compiled, Direction, Outcome, StreamScanner};
use crate::snapshot::{SnapExternal, SnapGuardrail};

/// Text above this many bytes is scanned on a blocking thread, so a large
/// prompt does not hold a runtime worker.
pub const INLINE_LIMIT_BYTES: usize = 64 * 1024;

/// How long a scan of a large text waits for a CPU slot when the caller gives
/// no deadline of its own.
const SCAN_WAIT: Duration = Duration::from_secs(10);

/// The blocking scans that may run at once in this process: as many as there
/// are CPUs, so a burst of large bodies cannot pin every core (and every other
/// team's calls) for as long as it takes to scan them all.
static SCAN_SLOTS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| {
    let cpus = std::thread::available_parallelism().map_or(2, std::num::NonZero::get);
    Arc::new(Semaphore::new(cpus))
});

/// How to reach the external guardrails of a call.
#[derive(Clone)]
pub struct Hooks {
    pub http: reqwest::Client,
    pub meta: Arc<CallMeta>,
    pub gates: Arc<HookGates>,
}

/// The guardrails a call is checked with, ready to run.
///
/// Rules guardrails run first, in order, over the original texts (a block
/// ends the check); external guardrails run after them, in order, and are
/// shown the texts as the rules left them.
#[derive(Clone, Default)]
pub struct Active {
    rules: Vec<Arc<Compiled>>,
    externals: Vec<Arc<SnapGuardrail>>,
    refs: Vec<GuardrailRef>,
    hooks: Option<Hooks>,
    scan_slots: Option<Arc<Semaphore>>,
}

impl Active {
    /// This set with its own pool of scan slots (the process-wide one is
    /// used otherwise).
    #[cfg(test)]
    pub(crate) fn with_scan_slots(mut self, slots: Arc<Semaphore>) -> Self {
        self.scan_slots = Some(slots);
        self
    }

    /// The running part of an effective set (defaults, route, key), in order.
    /// `hooks` is how external guardrails are reached; without it they are
    /// not part of the set.
    pub fn of(effective: &[Arc<SnapGuardrail>], hooks: Option<Hooks>) -> Self {
        let mut active = Active::default();
        for g in effective {
            if let Some(rules) = &g.rules {
                active.rules.push(rules.clone());
            } else if g.external.is_some() && hooks.is_some() {
                active.externals.push(g.clone());
            } else {
                continue;
            }
            active.refs.push(GuardrailRef {
                id: g.id,
                name: g.name.clone(),
            });
        }
        active.hooks = hooks;
        active
    }

    fn covers_rules(&self, dir: Direction) -> bool {
        self.rules.iter().any(|c| c.applies(dir))
    }

    /// The external guardrails asked about `dir`.
    fn externals_for(
        &self,
        dir: Direction,
    ) -> impl Iterator<Item = (&Arc<SnapGuardrail>, &SnapExternal)> {
        self.externals
            .iter()
            .filter_map(|g| g.external.as_ref().map(|e| (g, e)))
            .filter(move |(_, e)| e.directions.covers(dir))
    }

    /// Whether some rule or external guardrail applies to `dir`.
    pub fn covers(&self, dir: Direction) -> bool {
        self.covers_rules(dir) || self.externals_for(dir).next().is_some()
    }

    /// Whether an external guardrail is asked about the output, which makes a
    /// stream wait for the whole answer.
    pub fn holds_streams(&self) -> bool {
        self.has_hooks(Direction::Output)
    }

    /// The guardrails in force, by id and name, for the log.
    pub fn refs(&self) -> &[GuardrailRef] {
        &self.refs
    }

    /// The ids in force, in order.
    pub fn ids(&self) -> Vec<i64> {
        self.refs.iter().map(|r| r.id).collect()
    }

    /// A scanner for a stream, or `None` when no rule applies to outputs.
    pub fn stream_scanner(&self) -> Option<StreamScanner> {
        self.covers_rules(Direction::Output)
            .then(|| StreamScanner::new(self.rules.clone()))
    }

    /// A buffer that cannot be checked in full (it grew past its cap):
    /// every external guardrail of the output fails, each by its own mode.
    pub fn fail_output_buffer(&self) -> Outcome {
        let mut outcome = Outcome::default();
        for (g, ext) in self.externals_for(Direction::Output) {
            external::fail(g, ext, external::Failure::BufferFull, &mut outcome);
        }
        outcome
    }

    /// Whether an external guardrail is asked about `dir`.
    pub fn has_hooks(&self, dir: Direction) -> bool {
        self.externals_for(dir).next().is_some()
    }

    /// Asks the external guardrails about `texts` (already seen by the
    /// rules) and applies their answers in place. Nothing is asked of an
    /// empty set of texts. No call runs past `deadline`: one that would is
    /// cut short and fails by its mode.
    pub async fn check_externals(
        &self,
        dir: Direction,
        texts: &mut [String],
        deadline: Option<tokio::time::Instant>,
    ) -> Outcome {
        let mut outcome = Outcome::default();
        let Some(hooks) = &self.hooks else {
            return outcome;
        };
        if texts.iter().all(String::is_empty) {
            return outcome;
        }
        for (g, ext) in self.externals_for(dir) {
            let limit = match deadline {
                Some(at) => ext
                    .timeout
                    .min(at.saturating_duration_since(tokio::time::Instant::now())),
                None => ext.timeout,
            };
            external::run(
                &hooks.http,
                &hooks.gates,
                g,
                ext,
                &hooks.meta,
                dir,
                texts,
                limit,
                &mut outcome,
            )
            .await;
            if outcome.blocked_by.is_some() {
                break;
            }
        }
        outcome
    }

    /// The rules over `slots`, redacting in place; a block leaves them as
    /// they were. A text above [`INLINE_LIMIT_BYTES`] is scanned on a
    /// blocking thread once a CPU slot is free; a call that finds none by
    /// `deadline` (ten seconds when it has none) is refused as busy. `Err`
    /// only when the scan itself failed or was refused, in which case the
    /// slots are left empty and the caller must refuse the call.
    pub async fn check_rules(
        &self,
        dir: Direction,
        slots: &mut [&mut String],
        deadline: Option<tokio::time::Instant>,
    ) -> Result<Outcome, ScanFailed> {
        if !self.covers_rules(dir) || slots.is_empty() {
            return Ok(Outcome::default());
        }
        let mut texts: Vec<String> = slots.iter_mut().map(|s| std::mem::take(&mut **s)).collect();
        let total: usize = texts.iter().map(String::len).sum();
        let outcome;
        if total > INLINE_LIMIT_BYTES {
            let rules = self.rules.clone();
            let slots_pool = self.scan_slots.as_ref().unwrap_or(&SCAN_SLOTS).clone();
            let until = deadline.unwrap_or_else(|| tokio::time::Instant::now() + SCAN_WAIT);
            // the slot stays taken until the scan is over, even if the call
            // that asked for it is dropped meanwhile
            let slot = match tokio::time::timeout_at(until, slots_pool.acquire_owned()).await {
                Ok(Ok(slot)) => slot,
                Ok(Err(_)) => return Err(ScanFailed::Failed),
                Err(_) => return Err(ScanFailed::Busy),
            };
            (texts, outcome) = tokio::task::spawn_blocking(move || {
                let outcome = check_texts(&rules, dir, &mut texts);
                drop(slot);
                (texts, outcome)
            })
            .await
            .map_err(|_| ScanFailed::Failed)?;
        } else {
            outcome = check_texts(&self.rules, dir, &mut texts);
        }
        for (slot, text) in slots.iter_mut().zip(texts) {
            **slot = text;
        }
        Ok(outcome)
    }

    /// The external guardrails over `slots`, in place.
    pub async fn check_hooks(
        &self,
        dir: Direction,
        slots: &mut [&mut String],
        deadline: Option<tokio::time::Instant>,
    ) -> Outcome {
        if !self.has_hooks(dir) || slots.is_empty() {
            return Outcome::default();
        }
        let mut texts: Vec<String> = slots.iter_mut().map(|s| std::mem::take(&mut **s)).collect();
        let outcome = self.check_externals(dir, &mut texts, deadline).await;
        for (slot, text) in slots.iter_mut().zip(texts) {
            **slot = text;
        }
        outcome
    }

    /// Checks the texts in `slots` and redacts them in place: the rules,
    /// then (unless a rule blocked) the external guardrails. A block leaves
    /// them as they were. `Err` only when the scan itself failed.
    pub async fn check(
        &self,
        dir: Direction,
        mut slots: Vec<&mut String>,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<Outcome, ScanFailed> {
        let mut outcome = self.check_rules(dir, &mut slots, deadline).await?;
        if outcome.blocked_by.is_none() {
            let asked = self.check_hooks(dir, &mut slots, deadline).await;
            outcome.merge(&asked);
        }
        Ok(outcome)
    }
}

/// The scan did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanFailed {
    /// Its thread panicked.
    Failed,
    /// No CPU slot was free before the deadline.
    Busy,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guardrails::{Action, Directions, Matcher, PiiType, RuleSpec};

    fn active(slots: Arc<Semaphore>) -> Active {
        let rule = RuleSpec {
            id: "p".into(),
            matcher: Matcher::Pii(vec![PiiType::Email]),
            action: Action::Redact,
            directions: Directions::Both,
        };
        let compiled = Compiled::compile(1, "g", &[rule]).expect("compiles");
        Active {
            rules: vec![Arc::new(compiled)],
            ..Active::default()
        }
        .with_scan_slots(slots)
    }

    fn big_text() -> String {
        format!(
            "{} mail bob@example.com",
            "word ".repeat(INLINE_LIMIT_BYTES)
        )
    }

    #[tokio::test]
    async fn a_large_scan_with_no_free_slot_is_refused_as_busy() {
        let slots = Arc::new(Semaphore::new(1));
        let held = slots.clone().acquire_owned().await.expect("a slot");
        let guard = active(slots.clone());
        let mut text = big_text();
        let until = tokio::time::Instant::now() + Duration::from_millis(100);
        let started = std::time::Instant::now();
        let refused = guard
            .check_rules(Direction::Input, &mut [&mut text], Some(until))
            .await;
        assert_eq!(refused.unwrap_err(), ScanFailed::Busy);
        assert!(started.elapsed() < Duration::from_secs(5));
        // the slot comes back when its holder lets go, and the scan runs
        drop(held);
        let mut text = big_text();
        let outcome = guard
            .check_rules(Direction::Input, &mut [&mut text], Some(until))
            .await
            .expect("scans");
        assert_eq!(outcome.redactions.get("EMAIL"), Some(&1));
        assert!(text.ends_with("mail [REDACTED:EMAIL]"));
        assert_eq!(slots.available_permits(), 1, "the slot is given back");
    }

    #[tokio::test]
    async fn a_small_text_needs_no_slot() {
        let slots = Arc::new(Semaphore::new(0));
        let guard = active(slots);
        let mut text = "mail bob@example.com".to_string();
        let outcome = guard
            .check_rules(Direction::Input, &mut [&mut text], None)
            .await
            .expect("scans inline");
        assert_eq!(text, "mail [REDACTED:EMAIL]");
        assert_eq!(outcome.redactions.get("EMAIL"), Some(&1));
    }
}
