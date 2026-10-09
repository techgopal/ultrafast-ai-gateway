//! Running guardrails over a call: the set that applies, and a check over the
//! text slots of a request or an answer.

use std::sync::Arc;

use super::log::GuardrailRef;
use super::{check_texts, Compiled, Direction, Outcome, StreamScanner};
use crate::snapshot::SnapGuardrail;

/// Text above this many bytes is scanned on a blocking thread, so a large
/// prompt does not hold a runtime worker.
pub const INLINE_LIMIT_BYTES: usize = 64 * 1024;

/// The guardrails a call is checked with, ready to run.
///
/// Only guardrails of kind `rules` run here. SEAM for external guardrails
/// (plan 14, task 4): they are in `SnapGuardrail::external` and are skipped
/// by [`Active::of`] until the call-out exists; the proxy asks `Active` for
/// every check, so adding them means a second list here and an `async` step
/// in [`Active::check`], not a change to the proxy.
#[derive(Clone, Default)]
pub struct Active {
    rules: Vec<Arc<Compiled>>,
    refs: Vec<GuardrailRef>,
}

impl Active {
    /// The running part of an effective set (defaults, route, key), in order.
    pub fn of(effective: &[Arc<SnapGuardrail>]) -> Self {
        let mut active = Active::default();
        for g in effective {
            if let Some(rules) = &g.rules {
                active.rules.push(rules.clone());
                active.refs.push(GuardrailRef {
                    id: g.id,
                    name: g.name.clone(),
                });
            }
        }
        active
    }

    /// Whether some rule applies to `dir`.
    pub fn covers(&self, dir: Direction) -> bool {
        self.rules.iter().any(|c| c.applies(dir))
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
        self.covers(Direction::Output)
            .then(|| StreamScanner::new(self.rules.clone()))
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
        let texts: Vec<String> = slots.iter_mut().map(|s| std::mem::take(&mut **s)).collect();
        let total: usize = texts.iter().map(String::len).sum();
        let (texts, outcome) = if total > INLINE_LIMIT_BYTES {
            let rules = self.rules.clone();
            tokio::task::spawn_blocking(move || {
                let mut texts = texts;
                let outcome = check_texts(&rules, dir, &mut texts);
                (texts, outcome)
            })
            .await
            .map_err(|_| ScanFailed)?
        } else {
            let mut texts = texts;
            let outcome = check_texts(&self.rules, dir, &mut texts);
            (texts, outcome)
        };
        for (slot, text) in slots.into_iter().zip(texts) {
            *slot = text;
        }
        Ok(outcome)
    }
}

/// The scan did not finish (its thread panicked).
#[derive(Debug)]
pub struct ScanFailed;
