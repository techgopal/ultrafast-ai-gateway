//! Running guardrails over a call: the set that applies, and a check over the
//! text slots of a request or an answer.

use std::sync::Arc;

use super::external::{self, CallMeta};
use super::log::GuardrailRef;
use super::{check_texts, Compiled, Direction, Outcome, StreamScanner};
use crate::snapshot::{SnapExternal, SnapGuardrail};

/// Text above this many bytes is scanned on a blocking thread, so a large
/// prompt does not hold a runtime worker.
pub const INLINE_LIMIT_BYTES: usize = 64 * 1024;

/// How to reach the external guardrails of a call.
#[derive(Clone)]
pub struct Hooks {
    pub http: reqwest::Client,
    pub meta: Arc<CallMeta>,
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
}

impl Active {
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
        self.externals_for(Direction::Output).next().is_some()
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

    /// Asks the external guardrails about `texts` (already seen by the
    /// rules) and applies their answers in place. Nothing is asked of an
    /// empty set of texts.
    pub async fn check_externals(&self, dir: Direction, texts: &mut [String]) -> Outcome {
        let mut outcome = Outcome::default();
        let Some(hooks) = &self.hooks else {
            return outcome;
        };
        if texts.iter().all(String::is_empty) {
            return outcome;
        }
        for (g, ext) in self.externals_for(dir) {
            external::run(&hooks.http, g, ext, &hooks.meta, dir, texts, &mut outcome).await;
            if outcome.blocked_by.is_some() {
                break;
            }
        }
        outcome
    }

    /// Checks the texts in `slots` and redacts them in place; a block leaves
    /// them as they were. `Err` only when the scan itself failed, in which
    /// case the slots are left empty and the caller must refuse the call.
    pub async fn check(
        &self,
        dir: Direction,
        mut slots: Vec<&mut String>,
    ) -> Result<Outcome, ScanFailed> {
        if !self.covers(dir) || slots.is_empty() {
            return Ok(Outcome::default());
        }
        let mut texts: Vec<String> = slots.iter_mut().map(|s| std::mem::take(&mut **s)).collect();
        let mut outcome = Outcome::default();
        if self.covers_rules(dir) {
            let total: usize = texts.iter().map(String::len).sum();
            if total > INLINE_LIMIT_BYTES {
                let rules = self.rules.clone();
                (texts, outcome) = tokio::task::spawn_blocking(move || {
                    let outcome = check_texts(&rules, dir, &mut texts);
                    (texts, outcome)
                })
                .await
                .map_err(|_| ScanFailed)?;
            } else {
                outcome = check_texts(&self.rules, dir, &mut texts);
            }
        }
        // A rule that blocks ends the check; the hooks are not asked.
        if outcome.blocked_by.is_none() {
            let asked = self.check_externals(dir, &mut texts).await;
            outcome.merge(&asked);
        }
        for (slot, text) in slots.into_iter().zip(texts) {
            *slot = text;
        }
        Ok(outcome)
    }
}

/// The scan did not finish (its thread panicked).
#[derive(Debug)]
pub struct ScanFailed;
