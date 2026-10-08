//! Spending budgets of `/v1`, set for the gateway, a team, a user or a key,
//! per UTC calendar period (day, week from Monday, month).
//!
//! A `block` budget refuses calls once its amount is spent; an `alert`
//! budget lets them through and writes one audit row per period. Spend is
//! counted in memory behind [`Budgets`] (so a shared store can replace it)
//! when the log writer prices a call, written to `budget_usage` every
//! [`FLUSH_INTERVAL`] and at shutdown, and rebuilt from `request_logs` at
//! start: the logs are the source of truth, `budget_usage` is a cache. A
//! call that is already running is never stopped, so a budget can be
//! overshot by what was in flight when it was reached.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use anyhow::Result;
use time::{Date, Duration as Span, OffsetDateTime};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::app::AppState;
use crate::limits::LimitScope;
use crate::logs::writer::Accountant;
use crate::store::UsageRow;
use crate::telemetry::RequestRecord;

/// How often counters are written to the database.
pub const FLUSH_INTERVAL: Duration = Duration::from_secs(5);

/// How long a budget lasts before its counter starts again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Daily,
    /// From Monday 00:00 UTC.
    Weekly,
    Monthly,
}

impl Period {
    pub fn as_str(self) -> &'static str {
        match self {
            Period::Daily => "daily",
            Period::Weekly => "weekly",
            Period::Monthly => "monthly",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "daily" => Some(Period::Daily),
            "weekly" => Some(Period::Weekly),
            "monthly" => Some(Period::Monthly),
            _ => None,
        }
    }

    fn start(self, now: OffsetDateTime) -> Date {
        let day = now.to_offset(time::UtcOffset::UTC).date();
        match self {
            Period::Daily => day,
            Period::Weekly => day - Span::days(i64::from(day.weekday().number_days_from_monday())),
            Period::Monthly => day.replace_day(1).unwrap_or(day),
        }
    }

    /// The UTC date the period of `now` began on, as `YYYY-MM-DD`.
    pub fn start_string(self, now: OffsetDateTime) -> String {
        let d = self.start(now);
        format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
    }

    /// Midnight UTC when the period of `now` ends and the next begins.
    pub fn next_start(self, now: OffsetDateTime) -> OffsetDateTime {
        let start = self.start(now);
        let next = match self {
            Period::Daily => start + Span::days(1),
            Period::Weekly => start + Span::days(7),
            Period::Monthly => {
                let (year, month) = (start.year(), start.month());
                let year = if month == time::Month::December {
                    year + 1
                } else {
                    year
                };
                Date::from_calendar_date(year, month.next(), 1).unwrap_or(start + Span::days(31))
            }
        };
        next.midnight().assume_utc()
    }
}

/// What a budget does when its amount is spent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetAction {
    Block,
    Alert,
}

impl BudgetAction {
    pub fn as_str(self) -> &'static str {
        match self {
            BudgetAction::Block => "block",
            BudgetAction::Alert => "alert",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "block" => Some(BudgetAction::Block),
            "alert" => Some(BudgetAction::Alert),
            _ => None,
        }
    }
}

/// The budget of one subject for one period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    pub id: i64,
    pub scope: LimitScope,
    /// 0 for the gateway.
    pub scope_id: i64,
    /// How a message names the subject: `gateway`, `team 'Platform'`, ...
    pub scope_label: String,
    pub amount_micros: u64,
    pub period: Period,
    pub action: BudgetAction,
}

impl Budget {
    /// `monthly $50.00`.
    pub fn name(&self) -> String {
        format!("{} {}", self.period.as_str(), usd(self.amount_micros))
    }
}

/// Millionths of a dollar as dollars: at least two decimals, more only
/// when they are not zero (`$50.00`, `$0.001234`).
pub fn usd(micros: u64) -> String {
    let (whole, frac) = (micros / 1_000_000, micros % 1_000_000);
    let frac = format!("{frac:06}");
    let frac = frac.trim_end_matches('0');
    format!("${whole}.{frac:0<2}")
}

/// Why a call was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetRefusal {
    /// `monthly $50.00`.
    pub budget: String,
    /// The subject whose budget was reached, as its label.
    pub scope_label: String,
    /// Until the period resets.
    pub retry_after: Duration,
}

impl BudgetRefusal {
    pub fn message(&self) -> String {
        format!("budget '{}' of {} reached", self.budget, self.scope_label)
    }

    /// For the `Retry-After` header: whole seconds until the period
    /// resets, at least one.
    pub fn retry_after_seconds(&self) -> u64 {
        let d = self.retry_after;
        (d.as_secs() + u64::from(d.subsec_nanos() > 0)).max(1)
    }
}

/// A budget of an `alert` kind that reached its amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub budget_id: i64,
    pub period_start: String,
    pub spent_micros: u64,
    pub summary: String,
}

/// What changed since the last [`Budgets::drain`].
#[derive(Debug, Default)]
pub struct Drained {
    pub usage: Vec<UsageRow>,
    pub alerts: Vec<Alert>,
}

impl Drained {
    pub fn is_empty(&self) -> bool {
        self.usage.is_empty() && self.alerts.is_empty()
    }
}

/// Spend counters. `check` and `spend` never wait on the database.
pub trait Budgets: Send + Sync {
    /// Whether a call by a subject with these budgets may run: a `block`
    /// budget whose amount is spent in its current period refuses it. Of
    /// several, the longest wait is told.
    fn check(&self, budgets: &[Arc<Budget>], now: OffsetDateTime) -> Result<(), BudgetRefusal>;

    /// Adds the cost of a priced call to every budget, and raises the alert
    /// of an `alert` budget that reaches its amount for the first time in
    /// its period.
    fn spend(&self, budgets: &[Arc<Budget>], micros: u64, now: OffsetDateTime);

    /// What the budget has spent in the period of `now`.
    fn spent(&self, budget: &Budget, now: OffsetDateTime) -> u64;

    /// Merges what a budget had spent in a period (from the logs or the
    /// cache) into its counter: the larger of the two is kept, and a counter
    /// that was alerted stays alerted. An `alert` budget over its amount that was not
    /// alerted raises its alert.
    fn seed(&self, budget: &Budget, period_start: &str, spent_micros: u64, alerted: bool);

    /// Drops the counter of a deleted budget.
    fn forget(&self, budget_id: i64);

    /// Drops the counters of budgets that are not in `ids`.
    fn retain(&self, ids: &[i64]);

    /// Hands out the counters that changed and the alerts raised since the
    /// last call.
    fn drain(&self) -> Drained;

    /// Gives back what could not be written, for the next `drain`.
    fn requeue(&self, drained: Drained);
}

struct Counter {
    period_start: String,
    spent: u64,
    alerted: bool,
    dirty: bool,
}

#[derive(Default)]
struct Inner {
    counters: HashMap<i64, Counter>,
    alerts: Vec<Alert>,
}

/// Counters in the memory of this process. They are rebuilt from the
/// request logs at start and not shared between processes.
#[derive(Default)]
pub struct MemoryBudgets {
    inner: Mutex<Inner>,
}

impl MemoryBudgets {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // The counters stay usable after a panic elsewhere.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn alert_of(budget: &Budget, period_start: &str, spent: u64) -> Alert {
    Alert {
        budget_id: budget.id,
        period_start: period_start.to_string(),
        spent_micros: spent,
        summary: format!(
            "Budget '{} {}' reached {} of {}",
            budget.scope_label,
            budget.period.as_str(),
            usd(spent),
            usd(budget.amount_micros)
        ),
    }
}

impl Budgets for MemoryBudgets {
    fn check(&self, budgets: &[Arc<Budget>], now: OffsetDateTime) -> Result<(), BudgetRefusal> {
        // Strings are made before the lock is taken, which only reads.
        let blocking: Vec<(&Arc<Budget>, String)> = budgets
            .iter()
            .filter(|b| b.action == BudgetAction::Block)
            .map(|b| (b, b.period.start_string(now)))
            .collect();
        if blocking.is_empty() {
            return Ok(());
        }
        let spent: Vec<u64> = {
            let inner = self.lock();
            blocking
                .iter()
                .map(|(b, start)| {
                    inner
                        .counters
                        .get(&b.id)
                        .filter(|c| &c.period_start == start)
                        .map_or(0, |c| c.spent)
                })
                .collect()
        };
        let mut worst: Option<BudgetRefusal> = None;
        for ((b, _), spent) in blocking.iter().zip(spent) {
            if spent < b.amount_micros {
                continue;
            }
            let wait = (b.period.next_start(now) - now)
                .try_into()
                .unwrap_or_default();
            if worst.as_ref().is_none_or(|w| wait > w.retry_after) {
                worst = Some(BudgetRefusal {
                    budget: b.name(),
                    scope_label: b.scope_label.clone(),
                    retry_after: wait,
                });
            }
        }
        worst.map_or(Ok(()), Err)
    }

    fn spend(&self, budgets: &[Arc<Budget>], micros: u64, now: OffsetDateTime) {
        if micros == 0 {
            return;
        }
        let starts: Vec<String> = budgets.iter().map(|b| b.period.start_string(now)).collect();
        let mut guard = self.lock();
        let inner = &mut *guard;
        for (b, start) in budgets.iter().zip(starts) {
            // A call that began in a period that is over (it ran across the
            // turn) belongs to that period; the counter has moved on and the
            // logs hold it.
            if inner
                .counters
                .get(&b.id)
                .is_some_and(|c| c.period_start > start)
            {
                continue;
            }
            let counter = inner.counters.entry(b.id).or_insert_with(|| Counter {
                period_start: start.clone(),
                spent: 0,
                alerted: false,
                dirty: true,
            });
            if counter.period_start != start {
                *counter = Counter {
                    period_start: start.clone(),
                    spent: 0,
                    alerted: false,
                    dirty: true,
                };
            }
            counter.spent = counter.spent.saturating_add(micros);
            counter.dirty = true;
            if b.action == BudgetAction::Alert
                && !counter.alerted
                && counter.spent >= b.amount_micros
            {
                counter.alerted = true;
                inner.alerts.push(alert_of(b, &start, counter.spent));
            }
        }
    }

    fn spent(&self, budget: &Budget, now: OffsetDateTime) -> u64 {
        let start = budget.period.start_string(now);
        self.lock()
            .counters
            .get(&budget.id)
            .filter(|c| c.period_start == start)
            .map_or(0, |c| c.spent)
    }

    fn seed(&self, budget: &Budget, period_start: &str, spent_micros: u64, alerted: bool) {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let counter = match inner.counters.remove(&budget.id) {
            // Of the same period: merged, so a spend that landed before the
            // logs were read is not lost, nor is one the logs lack.
            Some(c) if c.period_start == period_start => Counter {
                spent: c.spent.max(spent_micros),
                alerted: c.alerted || alerted,
                dirty: true,
                ..c
            },
            // A counter of a later period is the live one.
            Some(c) if c.period_start.as_str() > period_start => c,
            _ => Counter {
                period_start: period_start.to_string(),
                spent: spent_micros,
                alerted,
                dirty: true,
            },
        };
        let mut counter = counter;
        if counter.period_start == period_start
            && budget.action == BudgetAction::Alert
            && !counter.alerted
            && counter.spent >= budget.amount_micros
        {
            counter.alerted = true;
            inner
                .alerts
                .push(alert_of(budget, period_start, counter.spent));
        }
        inner.counters.insert(budget.id, counter);
    }

    fn forget(&self, budget_id: i64) {
        let mut inner = self.lock();
        inner.counters.remove(&budget_id);
        inner.alerts.retain(|a| a.budget_id != budget_id);
    }

    fn retain(&self, ids: &[i64]) {
        let mut inner = self.lock();
        inner.counters.retain(|id, _| ids.contains(id));
        inner.alerts.retain(|a| ids.contains(&a.budget_id));
    }

    fn drain(&self) -> Drained {
        let mut inner = self.lock();
        let usage = inner
            .counters
            .iter_mut()
            .filter(|(_, c)| c.dirty)
            .map(|(id, c)| {
                c.dirty = false;
                UsageRow {
                    budget_id: *id,
                    period_start: c.period_start.clone(),
                    spent_micros: c.spent,
                }
            })
            .collect();
        Drained {
            usage,
            alerts: std::mem::take(&mut inner.alerts),
        }
    }

    fn requeue(&self, drained: Drained) {
        let mut inner = self.lock();
        for row in drained.usage {
            if let Some(c) = inner.counters.get_mut(&row.budget_id) {
                if c.period_start == row.period_start {
                    c.dirty = true;
                }
            }
        }
        inner.alerts.extend(drained.alerts);
    }
}

/// Counts the cost of a priced record against the budgets of its key, its
/// owner, their teams and the gateway.
pub fn account(state: &AppState, record: &RequestRecord, micros: u64, now: OffsetDateTime) {
    let budgets = state
        .snapshot
        .load()
        .budgets_of(record.key_id, record.user_id, record.team_id);
    if !budgets.is_empty() {
        state.budgets.spend(&budgets, micros, now);
    }
}

/// What the log writer calls for every record it priced.
pub fn accountant(state: Arc<AppState>) -> Accountant {
    Arc::new(move |record, micros| {
        // The period the call began in, as `request_logs.at` has it, so the
        // live counters and the rebuild agree for a call across midnight.
        let at = crate::store::parse_timestamp(&record.started_at)
            .unwrap_or_else(OffsetDateTime::now_utc);
        account(&state, record, micros, at)
    })
}

/// Sets every counter to what the logs say was spent in its current period
/// (or what the cache says, when that is more: retention may have deleted
/// the older logs of a month). Call it before the first `/v1` call is served.
pub async fn rebuild(state: &AppState, now: OffsetDateTime) -> Result<()> {
    let snapshot = state.snapshot.load_full();
    for b in snapshot.all_budgets() {
        seed_from_logs(state, &b, now).await?;
    }
    Ok(())
}

/// Counts the spend of a budget that is new (or whose counter is not
/// there) from the logs of its period.
pub async fn seed_from_logs(state: &AppState, budget: &Budget, now: OffsetDateTime) -> Result<()> {
    let start = budget.period.start_string(now);
    let scope_id = (budget.scope != LimitScope::Gateway).then_some(budget.scope_id);
    let logged = state
        .store
        .spend_since(budget.scope, scope_id, &format!("{start} 00:00:00"))
        .await?;
    let (cached, alerted) = state
        .store
        .budget_usage(budget.id, &start)
        .await?
        .unwrap_or((0, false));
    state
        .budgets
        .seed(budget, &start, logged.max(cached), alerted);
    Ok(())
}

/// Writes what changed to `budget_usage`, and the alerts raised to the
/// audit log. What cannot be written is tried again next time.
pub async fn flush(state: &AppState) {
    let drained = state.budgets.drain();
    if drained.is_empty() {
        return;
    }
    if let Err(e) = state.store.write_budget_usage(&drained.usage).await {
        tracing::warn!(error = %e, "could not write budget usage");
        state.budgets.requeue(drained);
        return;
    }
    // Tell the alert rules what was spent; they decide whether a threshold
    // was reached. Only after the counters are safe in the database.
    if let Some(engine) = &state.alert_engine {
        if !drained.usage.is_empty() {
            let budgets = state.snapshot.load().all_budgets();
            for row in &drained.usage {
                if let Some(budget) = budgets.iter().find(|b| b.id == row.budget_id) {
                    engine.spend(budget.clone(), row.period_start.clone(), row.spent_micros);
                }
            }
        }
    }
    for alert in drained.alerts {
        let written = state
            .store
            .record_budget_alert(
                alert.budget_id,
                &alert.period_start,
                alert.spent_micros,
                &alert.summary,
            )
            .await;
        if let Err(e) = written {
            tracing::warn!(error = %e, "could not write a budget alert");
            state.budgets.requeue(Drained {
                usage: Vec::new(),
                alerts: vec![alert],
            });
        }
    }
}

/// Flushes every `interval` and once more when `stop` becomes true (or its
/// sender is dropped), then ends.
pub fn spawn_flush(
    state: Arc<AppState>,
    interval: Duration,
    mut stop: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = tokio::time::sleep(interval) => flush(&state).await,
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() {
                        flush(&state).await;
                        return;
                    }
                }
            }
        }
    })
}
