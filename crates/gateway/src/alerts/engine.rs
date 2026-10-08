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

use anyhow::Result;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use super::errors_window::{ErrorWindows, Sample, Scope, Totals};
use super::rules::{self, rate_subject, ErrorRate, Params};
use super::Deliverer;
use crate::budgets::{usd, Budget};
use crate::routing::HealthEvent;
use crate::store::{now, NewAlertEvent, Store};

pub const INPUT_CAPACITY: usize = 1024;
pub const HEALTH_CAPACITY: usize = 256;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// How often error rates are evaluated.
    pub tick: Duration,
    /// How long a window bucket is: a minute, except in tests.
    pub bucket: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            tick: Duration::from_secs(30),
            bucket: Duration::from_secs(60),
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
    /// Counts a finished call. Never waits.
    pub fn observe(&self, sample: &Sample<'_>) {
        self.windows.record(sample);
    }

    /// A budget's spend. Never waits; a full queue drops it (the next flush
    /// says it again).
    pub fn spend(&self, budget: Arc<Budget>, period_start: String, spent_micros: u64) {
        let _ = self.tx.try_send(EngineInput::BudgetSpend {
            budget,
            period_start,
            spent_micros,
        });
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
            let ep = old.remove(&key).unwrap_or(Episode { below_since: None });
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
                self.map.insert(key, Episode { below_since: None });
            }
            Change::Resolve => {
                self.map.remove(&key);
            }
        }
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
    rules: Vec<Loaded>,
    episodes: Episodes,
}

impl Engine {
    pub fn new(store: Store, deliverer: Option<Deliverer>, windows: Arc<ErrorWindows>) -> Self {
        Self {
            store,
            deliverer,
            windows,
            rules: Vec::new(),
            episodes: Episodes::default(),
        }
    }

    /// Reads the rules and what they are firing for from the database.
    pub async fn load(&mut self) -> Result<()> {
        let rows = self.store.list_alert_rules().await?;
        let mut rules = Vec::new();
        for row in rows {
            let parsed = serde_json::from_str::<Value>(&row.params)
                .map_err(|e| e.to_string())
                .and_then(|v| rules::parse(&row.kind, &v));
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
                tx.upsert_alert_state(id, subject, &at).await?;
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
            let firing = self.episodes.is_firing(r.id, &subject);
            if !r.enabled || !matches || firing == opened {
                continue;
            }
            let (change, summary) = if opened {
                (
                    Change::Fire,
                    format!("Circuit opened for {provider}/{model}: calls to it are refused"),
                )
            } else {
                (
                    Change::Resolve,
                    format!("Circuit closed for {provider}/{model}: calls to it pass again"),
                )
            };
            let details = json!({ "provider": provider, "model": model });
            if let Err(e) = self
                .change(i, &subject, change, &summary, &details, None)
                .await
            {
                tracing::warn!(error = %e, "could not record an alert");
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

    /// Evaluates every error-rate rule against the windows at bucket `now`.
    pub async fn on_tick(&mut self, now: u32) {
        self.resolve_removed_targets().await;
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
        let mut engine = Engine::new(store, deliverer, windows.clone());
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
        let opened = HealthEvent::Opened {
            provider: "openai".into(),
            model: "gpt-4o".into(),
        };
        let closed = HealthEvent::Closed {
            provider: "openai".into(),
            model: "gpt-4o".into(),
        };
        e.on_health(&closed).await;
        assert!(events(&store).await.is_empty(), "closing what never opened");
        e.on_health(&opened).await;
        e.on_health(&opened).await;
        e.on_health(&closed).await;
        e.on_health(&closed).await;
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
            route: "chat",
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
}
