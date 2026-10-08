//! The alert engine: one task that turns spend, breaker changes and error
//! windows into `firing` and `resolved` events.
//!
//! It never sits on a `/v1` call. Calls feed the [`ErrorWindows`] directly
//! (a short lock); budgets and breakers send it small messages that are
//! dropped when its queue is full. A change of state is written with its
//! event in one transaction, then handed to the [`Deliverer`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use time::OffsetDateTime;
use tokio::time::Instant;

use anyhow::Result;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use super::errors_window::{ErrorWindows, Sample, Scope, Totals};
use super::rules::{self, rate_subject, ErrorRate, Params};
use super::Deliverer;
use crate::budgets::{usd, Budget};
use crate::routing::{HealthEvent, HealthStore, TargetState};
use crate::store::{now, NewAlertEvent, Store};

pub const INPUT_CAPACITY: usize = 1024;
pub const HEALTH_CAPACITY: usize = 256;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// How often error rates are evaluated.
    pub tick: Duration,
    /// How long a window bucket is: a minute, except in tests.
    pub bucket: Duration,
    /// How long a breaker must stay closed before its episode resolves, so a
    /// target that flaps is one episode and not a pair of notices per flap.
    pub circuit_quiet: Duration,
}

/// The default of [`EngineConfig::circuit_quiet`].
pub const CIRCUIT_QUIET: Duration = Duration::from_secs(300);

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            tick: Duration::from_secs(30),
            bucket: Duration::from_secs(60),
            circuit_quiet: CIRCUIT_QUIET,
        }
    }
}

/// What the engine is told besides breaker changes.
pub enum EngineInput {
    /// A budget's spend in its current period, after a flush.
    BudgetSpend {
        budget: Arc<Budget>,
        period_start: String,
        spent_micros: u64,
    },
    /// Evaluate the error windows now; `done` is answered afterwards.
    Tick { done: Option<oneshot::Sender<()>> },
    /// Read the rules again; `done` is answered afterwards.
    Reload { done: Option<oneshot::Sender<()>> },
}

/// How the rest of the gateway talks to the engine. Cheap to clone.
#[derive(Clone)]
pub struct EngineHandle {
    tx: mpsc::Sender<EngineInput>,
    health: mpsc::Sender<HealthEvent>,
    windows: Arc<ErrorWindows>,
}

impl EngineHandle {
    /// Whether finished calls are counted at all (an enabled error-rate rule
    /// exists).
    pub fn is_active(&self) -> bool {
        self.windows.is_active()
    }

    /// Counts a finished call. Never waits.
    pub fn observe(&self, sample: &Sample<'_>) {
        self.windows.record(sample);
    }

    /// A budget's spend. Never waits. `false`: the queue was full and the
    /// spend was not taken; the caller keeps the counter dirty so the next
    /// flush says it again.
    #[must_use]
    pub fn spend(&self, budget: Arc<Budget>, period_start: String, spent_micros: u64) -> bool {
        self.tx
            .try_send(EngineInput::BudgetSpend {
                budget,
                period_start,
                spent_micros,
            })
            .is_ok()
    }

    /// A handle whose engine never reads: for a test of a full queue.
    #[cfg(test)]
    pub(crate) fn unread(capacity: usize) -> (Self, mpsc::Receiver<EngineInput>) {
        let (tx, rx) = mpsc::channel(capacity);
        let (health, _) = mpsc::channel(1);
        (
            Self {
                tx,
                health,
                windows: Arc::new(ErrorWindows::new(Duration::from_secs(60))),
            },
            rx,
        )
    }

    /// Where breaker changes go.
    pub fn health_sender(&self) -> mpsc::Sender<HealthEvent> {
        self.health.clone()
    }

    /// The rules changed: read them again. Returns when the engine has.
    pub async fn reload(&self) {
        let (done, wait) = oneshot::channel();
        if self
            .tx
            .send(EngineInput::Reload { done: Some(done) })
            .await
            .is_ok()
        {
            let _ = tokio::time::timeout(Duration::from_secs(5), wait).await;
        }
    }

    /// Evaluates the error windows now. Returns when that is done.
    pub async fn tick(&self) {
        let (done, wait) = oneshot::channel();
        if self
            .tx
            .send(EngineInput::Tick { done: Some(done) })
            .await
            .is_ok()
        {
            let _ = tokio::time::timeout(Duration::from_secs(5), wait).await;
        }
    }

    pub fn windows(&self) -> &Arc<ErrorWindows> {
        &self.windows
    }
}

/// What happens to an episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Fire,
    Resolve,
}

struct Episode {
    /// The bucket the rate was first below the threshold in, while firing.
    below_since: Option<u32>,
    /// A circuit episode: since when its breaker has stayed closed.
    closed_since: Option<Instant>,
}

impl Episode {
    fn new() -> Self {
        Self {
            below_since: None,
            closed_since: None,
        }
    }
}

/// Which `(rule, subject)` pairs are firing. The state machine: an episode
/// begins with one `Fire` and ends with one `Resolve`; nothing is said in
/// between.
#[derive(Default)]
pub struct Episodes {
    map: HashMap<(i64, String), Episode>,
}

impl Episodes {
    /// Replaces what is firing with `rows` (rule, subject), keeping how long
    /// an episode that goes on has been below its threshold.
    pub fn replace<I: IntoIterator<Item = (i64, String)>>(&mut self, rows: I) {
        let mut old = std::mem::take(&mut self.map);
        for key in rows {
            let ep = old.remove(&key).unwrap_or_else(Episode::new);
            self.map.insert(key, ep);
        }
    }

    pub fn is_firing(&self, rule: i64, subject: &str) -> bool {
        self.map.contains_key(&(rule, subject.to_string()))
    }

    pub fn firing_subjects(&self, rule: i64) -> Vec<String> {
        self.map
            .keys()
            .filter(|(r, _)| *r == rule)
            .map(|(_, s)| s.clone())
            .collect()
    }

    /// An error rate was measured. A rate at or over the threshold fires; a
    /// firing episode resolves only after the rate stayed under it for a
    /// whole `window` of buckets, so a rate that hovers does not repeat.
    pub fn observe_rate(
        &mut self,
        rule: i64,
        subject: &str,
        breach: bool,
        now: u32,
        window: u32,
    ) -> Option<Change> {
        match self.map.get_mut(&(rule, subject.to_string())) {
            None => breach.then_some(Change::Fire),
            Some(ep) if breach => {
                ep.below_since = None;
                None
            }
            Some(ep) => {
                let since = *ep.below_since.get_or_insert(now);
                (now.saturating_sub(since) >= window).then_some(Change::Resolve)
            }
        }
    }

    fn apply(&mut self, rule: i64, subject: &str, change: Change) {
        let key = (rule, subject.to_string());
        match change {
            Change::Fire => {
                self.map.insert(key, Episode::new());
            }
            Change::Resolve => {
                self.map.remove(&key);
            }
        }
    }

    /// The breaker of a firing circuit episode is closed (as of `at`). The
    /// clock starts at the first report and is not moved by the next ones.
    fn mark_closed(&mut self, rule: i64, subject: &str, at: Instant) {
        if let Some(ep) = self.map.get_mut(&(rule, subject.to_string())) {
            ep.closed_since.get_or_insert(at);
        }
    }

    /// The breaker opened again: the episode goes on.
    fn mark_open(&mut self, rule: i64, subject: &str) {
        if let Some(ep) = self.map.get_mut(&(rule, subject.to_string())) {
            ep.closed_since = None;
        }
    }

    /// The firing subjects of a rule whose breaker has been closed for at
    /// least `quiet` as of `at`.
    fn quiet_subjects(&self, rule: i64, quiet: Duration, at: Instant) -> Vec<String> {
        self.map
            .iter()
            .filter(|((r, _), ep)| {
                *r == rule
                    && ep
                        .closed_since
                        .is_some_and(|since| at.saturating_duration_since(since) >= quiet)
            })
            .map(|((_, s), _)| s.clone())
            .collect()
    }

    /// Forgets an episode without a word (its state is dropped elsewhere).
    fn remove(&mut self, rule: i64, subject: &str) {
        self.map.remove(&(rule, subject.to_string()));
    }

    fn forget_other_periods(&mut self, rule: i64, prefix: &str, keep: &str) {
        self.map
            .retain(|(r, s), _| *r != rule || !s.starts_with(prefix) || s == keep);
    }
}

struct Loaded {
    id: i64,
    name: String,
    kind: String,
    enabled: bool,
    params: Params,
}

pub struct Engine {
    store: Store,
    deliverer: Option<Deliverer>,
    windows: Arc<ErrorWindows>,
    /// What the breakers say now, to settle circuit episodes whose events
    /// were lost.
    health: Option<Arc<dyn HealthStore>>,
    rules: Vec<Loaded>,
    episodes: Episodes,
    /// See [`EngineConfig::circuit_quiet`].
    quiet: Duration,
    /// The wall clock, for the budget periods (a field so a test sets it).
    wall: fn() -> OffsetDateTime,
}

impl Engine {
    pub fn new(
        store: Store,
        deliverer: Option<Deliverer>,
        windows: Arc<ErrorWindows>,
        health: Option<Arc<dyn HealthStore>>,
    ) -> Self {
        Self {
            store,
            deliverer,
            windows,
            health,
            rules: Vec::new(),
            episodes: Episodes::default(),
            quiet: CIRCUIT_QUIET,
            wall: OffsetDateTime::now_utc,
        }
    }

    /// Reads the rules and what they are firing for from the database.
    pub async fn load(&mut self) -> Result<()> {
        let rows = self.store.list_alert_rules().await?;
        let mut rules = Vec::new();
        for row in rows {
            let parsed = serde_json::from_str::<Value>(&row.params)
                .map_err(|_| ())
                .and_then(|v| rules::parse(&row.kind, &v).map_err(|_| ()));
            match parsed {
                Ok(params) => rules.push(Loaded {
                    id: row.id,
                    name: row.name,
                    kind: row.kind,
                    enabled: row.enabled,
                    params,
                }),
                Err(_) => tracing::warn!(
                    rule = row.id,
                    "an alert rule has parameters that cannot be read"
                ),
            }
        }
        let states = self.store.alert_states().await?;
        // Calls are counted only while a rule reads them.
        self.windows.set_active(
            rules
                .iter()
                .any(|r| r.enabled && matches!(r.params, Params::ErrorRate(_))),
        );
        self.rules = rules;
        self.episodes
            .replace(states.into_iter().map(|s| (s.rule_id, s.subject)));
        Ok(())
    }

    /// Writes the change and its event in one transaction, then queues the
    /// delivery.
    async fn change(
        &mut self,
        rule: usize,
        subject: &str,
        change: Change,
        summary: &str,
        details: &Value,
        keep_only_period: Option<&str>,
    ) -> Result<()> {
        let (id, name, kind) = {
            let r = &self.rules[rule];
            (r.id, r.name.clone(), r.kind.clone())
        };
        let at = now();
        let details_text = details.to_string();
        let mut tx = self.store.begin().await?;
        match change {
            Change::Fire => {
                if !tx.upsert_alert_state(id, subject, &at).await? {
                    // The rule was disabled or deleted since the engine read
                    // it: nothing to record.
                    return Ok(());
                }
                if let Some(prefix) = keep_only_period {
                    tx.delete_alert_states_except(id, prefix, subject).await?;
                }
            }
            Change::Resolve => tx.delete_alert_state(id, subject).await?,
        }
        let event_id = tx
            .insert_alert_event(NewAlertEvent {
                rule_id: Some(id),
                rule_name: &name,
                kind: &kind,
                subject,
                state: if change == Change::Fire {
                    "firing"
                } else {
                    "resolved"
                },
                summary,
                details: &details_text,
                at: &at,
            })
            .await?;
        tx.commit().await?;
        self.episodes.apply(id, subject, change);
        if let Some(prefix) = keep_only_period {
            self.episodes.forget_other_periods(id, prefix, subject);
        }
        if let Some(deliverer) = &self.deliverer {
            let channels = self.store.enabled_channel_ids_of_rule(id).await?;
            if !channels.is_empty() {
                deliverer.offer(event_id, channels);
            }
        }
        Ok(())
    }

    /// A budget's spend after a flush: fires the rules whose threshold it
    /// reached, once per period.
    pub async fn on_spend(&mut self, budget: &Budget, period_start: &str, spent: u64) {
        let amount = u128::from(budget.amount_micros);
        if amount == 0 {
            return;
        }
        let subject = format!("budget:{}:{period_start}", budget.id);
        let prefix = format!("budget:{}:", budget.id);
        for i in 0..self.rules.len() {
            let r = &self.rules[i];
            let Params::Budget { budget_id, percent } = &r.params else {
                continue;
            };
            if !r.enabled
                || budget_id.is_some_and(|id| id != budget.id)
                || u128::from(spent) * 100 < u128::from(*percent) * amount
                || self.episodes.is_firing(r.id, &subject)
            {
                continue;
            }
            let percent = *percent;
            let summary = format!(
                "Budget '{} {}' passed {percent}% ({} of {})",
                budget.scope_label,
                budget.period.as_str(),
                usd(spent),
                usd(budget.amount_micros)
            );
            let details = json!({
                "budget_id": budget.id,
                "scope": budget.scope.as_str(),
                "scope_id": budget.scope_id,
                "period": budget.period.as_str(),
                "period_start": period_start,
                "amount_micros": budget.amount_micros,
                "spent_micros": spent,
                "percent": percent,
            });
            if let Err(e) = self
                .change(i, &subject, Change::Fire, &summary, &details, Some(&prefix))
                .await
            {
                tracing::warn!(error = %e, "could not record an alert");
            }
        }
    }

    /// A breaker opened or closed.
    pub async fn on_health(&mut self, event: &HealthEvent) {
        self.on_health_at(event, Instant::now()).await;
    }

    /// A breaker report as of `at`. An opening starts an episode or, inside
    /// one, goes on with it; a closing only starts the quiet-period clock:
    /// [`Engine::settle_quiet_circuits`] resolves.
    async fn on_health_at(&mut self, event: &HealthEvent, at: Instant) {
        let (provider, model, opened) = match event {
            HealthEvent::Opened { provider, model } => (provider, model, true),
            HealthEvent::Closed { provider, model } => (provider, model, false),
        };
        let subject = format!("target:{provider}/{model}");
        for i in 0..self.rules.len() {
            let r = &self.rules[i];
            let Params::Circuit(c) = &r.params else {
                continue;
            };
            let matches = c.provider.as_ref().is_none_or(|p| p == provider)
                && c.model.as_ref().is_none_or(|m| m == model);
            if !r.enabled || !matches {
                continue;
            }
            let id = r.id;
            if self.episodes.is_firing(id, &subject) {
                if opened {
                    self.episodes.mark_open(id, &subject);
                } else {
                    self.episodes.mark_closed(id, &subject, at);
                }
                continue;
            }
            if !opened {
                continue;
            }
            let summary = format!("Circuit opened for {provider}/{model}: calls to it are refused");
            let details = json!({ "provider": provider, "model": model });
            if let Err(e) = self
                .change(i, &subject, Change::Fire, &summary, &details, None)
                .await
            {
                tracing::warn!(error = %e, "could not record an alert");
            }
        }
    }

    /// Resolves the circuit episodes whose breaker has stayed closed for the
    /// quiet period.
    async fn settle_quiet_circuits(&mut self, at: Instant) {
        for i in 0..self.rules.len() {
            let r = &self.rules[i];
            if !r.enabled || !matches!(r.params, Params::Circuit(_)) {
                continue;
            }
            for subject in self.episodes.quiet_subjects(r.id, self.quiet, at) {
                let Some((provider, model)) = subject
                    .strip_prefix("target:")
                    .and_then(|s| s.split_once('/'))
                else {
                    continue;
                };
                let summary =
                    format!("Circuit closed for {provider}/{model}: calls to it pass again");
                let details = json!({ "provider": provider, "model": model });
                if let Err(e) = self
                    .change(i, &subject, Change::Resolve, &summary, &details, None)
                    .await
                {
                    tracing::warn!(error = %e, "could not record an alert");
                }
            }
        }
    }

    /// Resolves the circuit episodes whose target is no longer in the
    /// catalog: a deleted model or provider never reports its breaker
    /// closing, so without this the episode would fire for ever.
    pub async fn resolve_removed_targets(&mut self) {
        let mut firing = Vec::new();
        for (i, r) in self.rules.iter().enumerate() {
            if r.enabled && matches!(r.params, Params::Circuit(_)) {
                for subject in self.episodes.firing_subjects(r.id) {
                    firing.push((i, subject));
                }
            }
        }
        if firing.is_empty() {
            return;
        }
        let known: std::collections::HashSet<(String, String)> =
            match self.store.list_models().await {
                Ok(models) => models
                    .into_iter()
                    .map(|m| (m.provider_name, m.name))
                    .collect(),
                Err(e) => {
                    tracing::warn!(error = %e, "could not read the catalog for the alert engine");
                    return;
                }
            };
        for (i, subject) in firing {
            let Some((provider, model)) = subject
                .strip_prefix("target:")
                .and_then(|s| s.split_once('/'))
            else {
                continue;
            };
            if known.contains(&(provider.to_string(), model.to_string())) {
                continue;
            }
            let details = json!({ "provider": provider, "model": model });
            if let Err(e) = self
                .change(
                    i,
                    &subject,
                    Change::Resolve,
                    "target removed",
                    &details,
                    None,
                )
                .await
            {
                tracing::warn!(error = %e, "could not record an alert");
            }
        }
    }

    /// Drops, silently, the budget episodes that are not of their budget's
    /// current period (or whose budget is gone): they would show as firing
    /// for ever. Budget rules send no resolved notice.
    async fn prune_budget_states(&mut self) {
        let mut stale = Vec::new();
        for r in &self.rules {
            if matches!(r.params, Params::Budget { .. }) {
                for subject in self.episodes.firing_subjects(r.id) {
                    stale.push((r.id, subject));
                }
            }
        }
        if stale.is_empty() {
            return;
        }
        let budgets = match self.store.list_budgets().await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %e, "could not read the budgets for the alert engine");
                return;
            }
        };
        let wall = (self.wall)();
        stale.retain(|(_, subject)| {
            let current = subject
                .strip_prefix("budget:")
                .and_then(|s| s.split_once(':'))
                .and_then(|(id, start)| Some((id.parse::<i64>().ok()?, start)))
                .is_some_and(|(id, start)| {
                    budgets
                        .iter()
                        .any(|b| b.id == id && b.period.start_string(wall) == start)
                });
            !current
        });
        if stale.is_empty() {
            return;
        }
        let dropped = async {
            let mut tx = self.store.begin().await?;
            for (rule, subject) in &stale {
                tx.delete_alert_state(*rule, subject).await?;
            }
            tx.commit().await
        }
        .await;
        match dropped {
            Ok(()) => {
                for (rule, subject) in &stale {
                    self.episodes.remove(*rule, subject);
                }
            }
            Err(e) => tracing::warn!(error = %e, "could not drop old alert states"),
        }
    }

    /// Settles circuit episodes against what the breakers say now: an open
    /// target without an episode fires, an episode whose target is closed
    /// starts its quiet period. A lost event is mended within a tick.
    async fn resync_circuits(&mut self, at: Instant) {
        let Some(health) = self.health.clone() else {
            return;
        };
        for t in health.view() {
            let (provider, model) = (t.provider, t.model);
            let event = if t.state == TargetState::Closed {
                HealthEvent::Closed { provider, model }
            } else {
                HealthEvent::Opened { provider, model }
            };
            self.on_health_at(&event, at).await;
        }
    }

    /// Evaluates every error-rate rule against the windows at bucket `now`.
    pub async fn on_tick(&mut self, now: u32) {
        self.on_tick_at(now, Instant::now()).await;
    }

    async fn on_tick_at(&mut self, now: u32, at: Instant) {
        self.resolve_removed_targets().await;
        self.resync_circuits(at).await;
        self.settle_quiet_circuits(at).await;
        self.prune_budget_states().await;
        self.windows.prune(now);
        for i in 0..self.rules.len() {
            let r = &self.rules[i];
            let Params::ErrorRate(p) = &r.params else {
                continue;
            };
            if !r.enabled {
                continue;
            }
            let (id, p) = (r.id, p.clone());
            self.evaluate_rate(i, id, &p, now).await;
        }
    }

    async fn evaluate_rate(&mut self, index: usize, id: i64, p: &ErrorRate, now: u32) {
        let window = p.window_minutes;
        let mut seen: Vec<(String, Totals)> = match (&p.subject, p.scope) {
            (_, Scope::Gateway) => vec![(
                "gateway".to_string(),
                self.windows.total_of(Scope::Gateway, "", now, window),
            )],
            (Some(subject), scope) => vec![(
                subject.clone(),
                self.windows.total_of(scope, subject, now, window),
            )],
            (None, scope) => self.windows.totals_of(scope, now, window),
        };
        // A subject that is firing but had no call lately still has to be able
        // to resolve.
        let prefix = format!("{}:", p.scope.as_str());
        for subject in self.episodes.firing_subjects(id) {
            let raw = if p.scope == Scope::Gateway {
                "gateway"
            } else {
                subject.strip_prefix(&prefix).unwrap_or(&subject)
            };
            if !seen.iter().any(|(s, _)| s == raw) {
                let totals = self.windows.total_of(p.scope, raw, now, window);
                seen.push((raw.to_string(), totals));
            }
        }
        for (raw, totals) in seen {
            let subject = rate_subject(p.scope, &raw);
            let breach = totals.breaches(p.percent, p.min_requests);
            let Some(change) = self
                .episodes
                .observe_rate(id, &subject, breach, now, window)
            else {
                continue;
            };
            let what = match p.scope {
                Scope::Gateway => "the gateway".to_string(),
                scope => format!("{} '{raw}'", scope.as_str()),
            };
            let share = format!("{:.1}%", totals.percent());
            let summary = match change {
                Change::Fire => format!(
                    "Error rate of {what} is {share} ({} of {} calls in {window} min), at or over {}%",
                    totals.errors, totals.requests, p.percent
                ),
                Change::Resolve => format!(
                    "Error rate of {what} is back under {}% ({share} over {window} min)",
                    p.percent
                ),
            };
            let details = json!({
                "scope": p.scope.as_str(),
                "subject": if p.scope == Scope::Gateway { Value::Null } else { json!(raw) },
                "percent": p.percent,
                "window_minutes": window,
                "min_requests": p.min_requests,
                "requests": totals.requests,
                "errors": totals.errors,
                "error_percent": (totals.percent() * 10.0).round() / 10.0,
            });
            if let Err(e) = self
                .change(index, &subject, change, &summary, &details, None)
                .await
            {
                tracing::warn!(error = %e, "could not record an alert");
            }
        }
    }
}

/// Starts the engine. It ends after `stop` turns true.
pub fn spawn(
    store: Store,
    deliverer: Option<Deliverer>,
    health: Option<Arc<dyn HealthStore>>,
    cfg: EngineConfig,
    mut stop: watch::Receiver<bool>,
) -> (EngineHandle, JoinHandle<()>) {
    let (tx, mut rx) = mpsc::channel(INPUT_CAPACITY);
    let (health_tx, mut health_rx) = mpsc::channel(HEALTH_CAPACITY);
    let windows = Arc::new(ErrorWindows::new(cfg.bucket));
    let handle = EngineHandle {
        tx,
        health: health_tx,
        windows: windows.clone(),
    };
    let task = tokio::spawn(async move {
        let mut engine = Engine::new(store, deliverer, windows.clone(), health);
        engine.quiet = cfg.circuit_quiet;
        if let Err(e) = engine.load().await {
            tracing::warn!(error = %e, "could not read the alert rules");
        }
        let mut ticker = tokio::time::interval(cfg.tick);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;
        loop {
            tokio::select! {
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() {
                        return;
                    }
                }
                input = rx.recv() => {
                    let Some(input) = input else { return };
                    match input {
                        EngineInput::BudgetSpend { budget, period_start, spent_micros } => {
                            engine.on_spend(&budget, &period_start, spent_micros).await;
                        }
                        EngineInput::Tick { done } => {
                            engine.on_tick(windows.current()).await;
                            if let Some(done) = done { let _ = done.send(()); }
                        }
                        EngineInput::Reload { done } => {
                            if let Err(e) = engine.load().await {
                                tracing::warn!(error = %e, "could not read the alert rules");
                            }
                            engine.resolve_removed_targets().await;
                            if let Some(done) = done { let _ = done.send(()); }
                        }
                    }
                }
                Some(event) = health_rx.recv() => engine.on_health(&event).await,
                _ = ticker.tick() => {
                    // Rules changed by the command line's import are picked
                    // up here; the API's writes reload at once.
                    if let Err(e) = engine.load().await {
                        tracing::warn!(error = %e, "could not read the alert rules");
                    }
                    engine.on_tick(windows.current()).await;
                }
            }
        }
    });
    (handle, task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budgets::{BudgetAction, Period};
    use crate::limits::LimitScope;

    fn episodes_with(rule: i64, subject: &str) -> Episodes {
        let mut e = Episodes::default();
        e.replace([(rule, subject.to_string())]);
        e
    }

    #[test]
    fn an_episode_fires_once_and_says_nothing_while_it_goes_on() {
        let mut e = Episodes::default();
        assert_eq!(
            e.observe_rate(1, "route:a", true, 10, 5),
            Some(Change::Fire)
        );
        e.apply(1, "route:a", Change::Fire);
        for now in 11..40 {
            assert_eq!(e.observe_rate(1, "route:a", true, now, 5), None, "{now}");
        }
        // Another subject and another rule are their own episodes.
        assert_eq!(
            e.observe_rate(1, "route:b", true, 11, 5),
            Some(Change::Fire)
        );
        assert_eq!(
            e.observe_rate(2, "route:a", true, 11, 5),
            Some(Change::Fire)
        );
        // Nothing fires while the rate is fine.
        assert_eq!(e.observe_rate(1, "route:c", false, 11, 5), None);
    }

    #[test]
    fn it_resolves_only_after_a_full_window_below() {
        let mut e = episodes_with(1, "route:a");
        assert_eq!(e.observe_rate(1, "route:a", false, 100, 5), None);
        for now in 101..105 {
            assert_eq!(e.observe_rate(1, "route:a", false, now, 5), None, "{now}");
        }
        assert_eq!(
            e.observe_rate(1, "route:a", false, 105, 5),
            Some(Change::Resolve)
        );
        e.apply(1, "route:a", Change::Resolve);
        assert!(!e.is_firing(1, "route:a"));
        // The next breach is a new episode.
        assert_eq!(
            e.observe_rate(1, "route:a", true, 106, 5),
            Some(Change::Fire)
        );
    }

    #[test]
    fn a_flapping_rate_is_one_episode() {
        let mut e = episodes_with(1, "route:a");
        let mut said = 0;
        // Below for 3 buckets, over for 1, again and again: never a full window.
        for now in 100..200u32 {
            let breach = now % 4 == 0;
            if e.observe_rate(1, "route:a", breach, now, 5).is_some() {
                said += 1;
            }
        }
        assert_eq!(said, 0, "still the one episode, nothing said");
        assert!(e.is_firing(1, "route:a"));
    }

    #[test]
    fn replace_keeps_the_progress_of_an_episode_that_goes_on() {
        let mut e = episodes_with(1, "route:a");
        assert_eq!(e.observe_rate(1, "route:a", false, 100, 5), None);
        e.replace([(1, "route:a".to_string())]);
        assert_eq!(
            e.observe_rate(1, "route:a", false, 105, 5),
            Some(Change::Resolve)
        );
        // A subject the database no longer has is gone.
        e.replace([]);
        assert!(!e.is_firing(1, "route:a"));
    }

    // ---- with a database

    fn budget(id: i64, amount: u64) -> Budget {
        Budget {
            id,
            scope: LimitScope::Team,
            scope_id: 4,
            scope_label: "team 'Search'".into(),
            amount_micros: amount,
            period: Period::Monthly,
            action: BudgetAction::Block,
        }
    }

    async fn rule(store: &Store, name: &str, kind: &str, params: Value) -> i64 {
        let mut tx = store.begin().await.unwrap();
        let id = tx
            .insert_alert_rule(name, kind, &params.to_string(), true)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        id
    }

    async fn engine(store: &Store) -> Engine {
        let mut e = Engine::new(
            store.clone(),
            None,
            Arc::new(ErrorWindows::new(Duration::from_secs(60))),
            None,
        );
        e.load().await.unwrap();
        e
    }

    async fn events(store: &Store) -> Vec<(String, String)> {
        let mut v: Vec<_> = store
            .alert_events(100)
            .await
            .unwrap()
            .into_iter()
            .map(|e| (e.state, e.subject))
            .collect();
        v.reverse();
        v
    }

    #[tokio::test]
    async fn a_budget_threshold_fires_once_per_period_and_not_again_after_a_restart() {
        let store = Store::open_in_memory().await.unwrap();
        let rule_id = rule(&store, "75", "budget", json!({ "percent": 75 })).await;
        let b = budget(9, 1_000_000);
        let mut e = engine(&store).await;
        e.on_spend(&b, "2999-01-01", 700_000).await;
        assert!(events(&store).await.is_empty(), "70% is under 75%");
        e.on_spend(&b, "2999-01-01", 800_000).await;
        e.on_spend(&b, "2999-01-01", 900_000).await;
        e.on_spend(&b, "2999-01-01", 1_000_000).await;
        assert_eq!(
            events(&store).await,
            [("firing".to_string(), "budget:9:2999-01-01".to_string())]
        );
        let ev = store.alert_events(1).await.unwrap().remove(0);
        let d: Value = serde_json::from_str(&ev.details).unwrap();
        assert_eq!(
            d,
            json!({ "budget_id": 9, "scope": "team", "scope_id": 4, "period": "monthly",
                    "period_start": "2999-01-01", "amount_micros": 1_000_000,
                    "spent_micros": 800_000, "percent": 75 })
        );
        assert_eq!(ev.rule_id, Some(rule_id));
        // A restart: a new engine reads the persisted state.
        let mut again = engine(&store).await;
        again.on_spend(&b, "2999-01-01", 950_000).await;
        assert_eq!(
            events(&store).await.len(),
            1,
            "no second event after a restart"
        );
        // A new period is a new subject; the old period's state is dropped.
        again.on_spend(&b, "2999-02-01", 760_000).await;
        assert_eq!(events(&store).await.len(), 2);
        let states = store.alert_states().await.unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].subject, "budget:9:2999-02-01");
        // Another budget with a rule for every budget fires on its own.
        again.on_spend(&budget(10, 100), "2999-02-01", 100).await;
        assert_eq!(events(&store).await.len(), 3);
    }

    #[tokio::test]
    async fn a_rule_for_one_budget_ignores_the_others_and_a_disabled_one_never_fires() {
        let store = Store::open_in_memory().await.unwrap();
        rule(
            &store,
            "one",
            "budget",
            json!({ "budget_id": 1, "percent": 50 }),
        )
        .await;
        let off = rule(&store, "off", "budget", json!({ "percent": 1 })).await;
        sqlx::query("UPDATE alert_rules SET enabled = 0 WHERE id = ?")
            .bind(off)
            .execute(store.pool())
            .await
            .unwrap();
        let mut e = engine(&store).await;
        e.on_spend(&budget(2, 100), "2999-01-01", 100).await;
        assert!(events(&store).await.is_empty());
        e.on_spend(&budget(1, 100), "2999-01-01", 50).await;
        assert_eq!(events(&store).await.len(), 1);
    }

    #[tokio::test]
    async fn a_breaker_fires_on_open_and_resolves_on_close_once_each() {
        let store = Store::open_in_memory().await.unwrap();
        rule(&store, "any", "circuit_open", json!({})).await;
        rule(
            &store,
            "other",
            "circuit_open",
            json!({ "provider": "anthropic" }),
        )
        .await;
        let mut e = engine(&store).await;
        e.quiet = Duration::ZERO;
        let opened = HealthEvent::Opened {
            provider: "openai".into(),
            model: "gpt-4o".into(),
        };
        let closed = HealthEvent::Closed {
            provider: "openai".into(),
            model: "gpt-4o".into(),
        };
        e.on_health(&closed).await;
        e.on_tick(1).await;
        assert!(events(&store).await.is_empty(), "closing what never opened");
        e.on_health(&opened).await;
        e.on_health(&opened).await;
        e.on_health(&closed).await;
        e.on_health(&closed).await;
        e.on_tick(2).await;
        e.on_tick(3).await;
        assert_eq!(
            events(&store).await,
            [
                ("firing".to_string(), "target:openai/gpt-4o".to_string()),
                ("resolved".to_string(), "target:openai/gpt-4o".to_string()),
            ]
        );
        assert!(store.alert_states().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_error_rate_fires_at_the_threshold_and_resolves_after_a_quiet_window() {
        let store = Store::open_in_memory().await.unwrap();
        rule(
            &store,
            "errs",
            "error_rate",
            json!({ "scope": "route", "percent": 50, "window_minutes": 5, "min_requests": 4 }),
        )
        .await;
        let mut e = engine(&store).await;
        let w = e.windows.clone();
        let call = |error| Sample {
            route: Some("chat"),
            provider: Some("p"),
            key_id: None,
            error,
        };
        // 3 requests: under the minimum, nothing.
        for _ in 0..3 {
            w.record_at(10, &call(true));
        }
        e.on_tick(10).await;
        assert!(events(&store).await.is_empty());
        w.record_at(10, &call(false));
        e.on_tick(10).await;
        e.on_tick(11).await;
        assert_eq!(
            events(&store).await,
            [("firing".to_string(), "route:chat".to_string())]
        );
        // The errors age out of the window at 15; then a full quiet window.
        for now in 12..=19 {
            e.on_tick(now).await;
        }
        assert_eq!(events(&store).await.len(), 1, "not yet");
        for now in 20..=21 {
            e.on_tick(now).await;
        }
        assert_eq!(
            events(&store).await,
            [
                ("firing".to_string(), "route:chat".to_string()),
                ("resolved".to_string(), "route:chat".to_string())
            ]
        );
        assert!(store.alert_states().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn circuit_episodes_are_settled_against_the_breakers_on_each_tick() {
        use crate::routing::{BreakerSettings, InMemoryHealth, TargetRef};
        use tokio::time::Instant;
        const S: BreakerSettings = BreakerSettings {
            failures: 2,
            window: Duration::from_secs(60),
            open: Duration::from_secs(30),
        };
        let store = Store::open_in_memory().await.unwrap();
        rule(&store, "any", "circuit_open", json!({})).await;
        let provider = store
            .insert_provider("p", "openai", "http://p.example", None)
            .await
            .unwrap();
        let mut tx = store.begin().await.unwrap();
        tx.insert_model(provider, "m").await.unwrap();
        tx.commit().await.unwrap();
        let health = Arc::new(InMemoryHealth::new());
        let mut e = Engine::new(
            store.clone(),
            None,
            Arc::new(ErrorWindows::new(Duration::from_secs(60))),
            Some(health.clone()),
        );
        e.load().await.unwrap();
        e.quiet = Duration::ZERO;
        // Nobody listens to the breaker: no event reaches the engine.
        let t = TargetRef {
            provider: "p".into(),
            model: "m".into(),
            model_id: 1,
        };
        let start = Instant::now();
        health.report(&t, false, true, Some(500), start, &S);
        e.on_tick(1).await;
        assert!(events(&store).await.is_empty(), "one failure: closed");
        health.report(&t, false, true, Some(500), start, &S);
        e.on_tick(2).await;
        e.on_tick(3).await;
        assert_eq!(
            events(&store).await,
            [("firing".to_string(), "target:p/m".to_string())],
            "an open target without an episode fires, once"
        );
        // Half open is still open; the trial succeeds and it closes.
        let later = start + Duration::from_secs(31);
        assert!(health.allow(&t, later, &S));
        e.on_tick(4).await;
        assert_eq!(events(&store).await.len(), 1);
        health.report(&t, true, false, Some(200), later, &S);
        e.on_tick(5).await;
        e.on_tick(6).await;
        assert_eq!(
            events(&store).await,
            [
                ("firing".to_string(), "target:p/m".to_string()),
                ("resolved".to_string(), "target:p/m".to_string())
            ]
        );
    }

    fn flap(provider: &str, model: &str, opened: bool) -> HealthEvent {
        let (provider, model) = (provider.to_string(), model.to_string());
        if opened {
            HealthEvent::Opened { provider, model }
        } else {
            HealthEvent::Closed { provider, model }
        }
    }

    #[tokio::test]
    async fn a_flapping_breaker_is_one_episode_that_resolves_after_a_quiet_period() {
        let store = Store::open_in_memory().await.unwrap();
        rule(&store, "any", "circuit_open", json!({})).await;
        let provider = store
            .insert_provider("p", "openai", "http://p.example", None)
            .await
            .unwrap();
        let mut tx = store.begin().await.unwrap();
        tx.insert_model(provider, "m").await.unwrap();
        tx.commit().await.unwrap();
        let mut e = engine(&store).await;
        assert_eq!(e.quiet, CIRCUIT_QUIET, "5 minutes unless configured");
        let t0 = Instant::now();
        let mut at = t0;
        // 10 flaps, 10 s apart, a tick after each close: nothing but the
        // first opening is said.
        for _ in 0..10 {
            e.on_health_at(&flap("p", "m", true), at).await;
            at += Duration::from_secs(10);
            e.on_health_at(&flap("p", "m", false), at).await;
            at += Duration::from_secs(10);
            e.on_tick_at(1, at).await;
        }
        assert_eq!(
            events(&store).await,
            [("firing".to_string(), "target:p/m".to_string())]
        );
        // Closed for a whole quiet period: one resolved, not before.
        e.on_health_at(&flap("p", "m", true), at).await;
        let closed_at = at + Duration::from_secs(1);
        e.on_health_at(&flap("p", "m", false), closed_at).await;
        e.on_tick_at(2, closed_at + Duration::from_secs(299)).await;
        assert_eq!(events(&store).await.len(), 1, "299 s: still quiet-pending");
        // Opening inside the period continues the episode and restarts it.
        e.on_health_at(&flap("p", "m", true), closed_at + Duration::from_secs(299))
            .await;
        let again = closed_at + Duration::from_secs(300);
        e.on_health_at(&flap("p", "m", false), again).await;
        e.on_tick_at(3, again + Duration::from_secs(299)).await;
        assert_eq!(events(&store).await.len(), 1);
        e.on_tick_at(4, again + Duration::from_secs(300)).await;
        assert_eq!(
            events(&store).await,
            [
                ("firing".to_string(), "target:p/m".to_string()),
                ("resolved".to_string(), "target:p/m".to_string())
            ]
        );
        assert!(store.alert_states().await.unwrap().is_empty());
        // The next opening is a new episode.
        e.on_health_at(&flap("p", "m", true), again + Duration::from_secs(301))
            .await;
        assert_eq!(events(&store).await.len(), 3);
    }

    async fn gateway_budget(store: &Store) -> i64 {
        let mut tx = store.begin().await.unwrap();
        let id = tx
            .upsert_budget(
                LimitScope::Gateway,
                None,
                1_000_000,
                Period::Monthly,
                BudgetAction::Block,
            )
            .await
            .unwrap();
        tx.commit().await.unwrap();
        id
    }

    fn in_january() -> OffsetDateTime {
        time::macros::datetime!(2999-01-15 12:00 UTC)
    }

    fn in_february() -> OffsetDateTime {
        time::macros::datetime!(2999-02-15 12:00 UTC)
    }

    #[tokio::test]
    async fn a_budget_episode_of_a_past_period_is_dropped_on_the_tick_without_a_word() {
        let store = Store::open_in_memory().await.unwrap();
        rule(&store, "80", "budget", json!({ "percent": 80 })).await;
        let id = gateway_budget(&store).await;
        let b = budget(id, 1_000_000);
        let mut e = engine(&store).await;
        e.wall = in_january;
        e.on_spend(&b, "2999-01-01", 900_000).await;
        e.on_tick(1).await;
        assert_eq!(store.alert_states().await.unwrap().len(), 1, "same period");
        e.wall = in_february;
        e.on_tick(2).await;
        assert!(store.alert_states().await.unwrap().is_empty());
        assert_eq!(events(&store).await.len(), 1, "no resolved notice");
        // And the memory forgot it too: the new period fires afresh.
        e.on_spend(&b, "2999-02-01", 900_000).await;
        assert_eq!(events(&store).await.len(), 2);
    }

    #[tokio::test]
    async fn a_budget_episode_of_a_deleted_budget_is_dropped_on_the_tick() {
        let store = Store::open_in_memory().await.unwrap();
        rule(&store, "80", "budget", json!({ "percent": 80 })).await;
        let id = gateway_budget(&store).await;
        let mut e = engine(&store).await;
        e.wall = in_january;
        e.on_spend(&budget(id, 1_000_000), "2999-01-01", 900_000)
            .await;
        let mut tx = store.begin().await.unwrap();
        assert!(tx.delete_budget(id).await.unwrap());
        tx.commit().await.unwrap();
        e.on_tick(1).await;
        assert!(store.alert_states().await.unwrap().is_empty());
        assert_eq!(events(&store).await.len(), 1);
    }

    #[tokio::test]
    async fn a_rule_disabled_while_the_engine_changes_it_leaves_no_state_and_no_event() {
        let store = Store::open_in_memory().await.unwrap();
        let rule_id = rule(&store, "80", "budget", json!({ "percent": 80 })).await;
        let mut e = engine(&store).await;
        // The API disables the rule (and clears its states) after the engine
        // read it and before the engine writes.
        sqlx::query("UPDATE alert_rules SET enabled = 0 WHERE id = ?")
            .bind(rule_id)
            .execute(store.pool())
            .await
            .unwrap();
        e.on_spend(&budget(9, 100), "2999-01-01", 100).await;
        assert!(store.alert_states().await.unwrap().is_empty());
        assert!(events(&store).await.is_empty());
        assert!(!e.episodes.is_firing(rule_id, "budget:9:2999-01-01"));
    }
}
