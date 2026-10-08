//! What a rule is about: the parameters of each kind, checked.
//!
//! A rule's `params` are JSON. They are checked when the rule is written
//! (unknown fields are refused) and again when the engine loads them.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::errors_window::Scope;

/// The kinds of rule, as stored.
pub const KINDS: [&str; 3] = ["budget", "error_rate", "circuit_open"];

pub const DEFAULT_WINDOW_MINUTES: i64 = 5;
pub const DEFAULT_MIN_REQUESTS: i64 = 20;
/// The longest a name in the parameters may be.
const MAX_NAME: usize = 200;

/// `budget`: fires once per budget period when the spend reaches `percent`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetParams {
    /// The budget; `null` or left out: every budget.
    #[serde(default)]
    pub budget_id: Option<i64>,
    /// 1 to 100.
    pub percent: i64,
}

/// `error_rate`: fires when, over the window, the share of calls that ended
/// in a server error (5xx, or 429 from the provider) reaches `percent`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorRateParams {
    /// `gateway`, `route`, `provider` or `key`.
    pub scope: String,
    /// The route or provider name, or the key id; `null` or left out: each
    /// subject of the scope on its own. Not used for `gateway`.
    #[serde(default)]
    pub subject: Option<String>,
    /// 1 to 100.
    pub percent: i64,
    /// 5 to 60; 5 when left out.
    #[serde(default = "default_window")]
    pub window_minutes: i64,
    /// 1 to 100000; 20 when left out. Fewer calls in the window never fire.
    #[serde(default = "default_min_requests")]
    pub min_requests: i64,
}

fn default_window() -> i64 {
    DEFAULT_WINDOW_MINUTES
}

fn default_min_requests() -> i64 {
    DEFAULT_MIN_REQUESTS
}

/// `circuit_open`: fires when a breaker opens, resolves when it closes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CircuitParams {
    /// `null` or left out: any provider.
    #[serde(default)]
    pub provider: Option<String>,
    /// `null` or left out: any model.
    #[serde(default)]
    pub model: Option<String>,
}

/// Parameters, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Params {
    Budget { budget_id: Option<i64>, percent: u8 },
    ErrorRate(ErrorRate),
    Circuit(CircuitParams),
}

/// `error_rate` parameters, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorRate {
    pub scope: Scope,
    pub subject: Option<String>,
    pub percent: u8,
    pub window_minutes: u32,
    pub min_requests: u32,
}

fn range(name: &str, value: i64, low: i64, high: i64) -> Result<i64, String> {
    if (low..=high).contains(&value) {
        Ok(value)
    } else {
        Err(format!("{name} must be from {low} to {high}"))
    }
}

fn name_ok(what: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.trim() != value {
        return Err(format!(
            "{what} must not be empty or start or end with a space"
        ));
    }
    if value.len() > MAX_NAME || value.chars().any(char::is_control) {
        return Err(format!(
            "{what} must be at most {MAX_NAME} bytes, without control characters"
        ));
    }
    Ok(())
}

/// The message names the problem and never repeats more than the field name.
pub fn parse(kind: &str, value: &Value) -> Result<Params, String> {
    fn read<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, String> {
        serde_json::from_value(value.clone()).map_err(|e| e.to_string())
    }
    match kind {
        "budget" => {
            let p: BudgetParams = read(value)?;
            if let Some(id) = p.budget_id {
                if id < 1 {
                    return Err("budget_id must be a budget id or null".to_string());
                }
            }
            let percent = range("percent", p.percent, 1, 100)?;
            Ok(Params::Budget {
                budget_id: p.budget_id,
                percent: percent as u8,
            })
        }
        "error_rate" => {
            let p: ErrorRateParams = read(value)?;
            let scope = Scope::parse(&p.scope)
                .ok_or("scope must be gateway, route, provider or key".to_string())?;
            let subject = match (&p.subject, scope) {
                (None, _) => None,
                (Some(_), Scope::Gateway) => {
                    return Err("subject is not used for the scope gateway".to_string())
                }
                (Some(s), Scope::Key) => {
                    if s.parse::<i64>().map_or(true, |id| id < 1) || s.starts_with('+') {
                        return Err("subject must be a key id for the scope key".to_string());
                    }
                    Some(s.clone())
                }
                (Some(s), _) => {
                    name_ok("subject", s)?;
                    Some(s.clone())
                }
            };
            let percent = range("percent", p.percent, 1, 100)?;
            let window = range("window_minutes", p.window_minutes, 5, 60)?;
            let min = range("min_requests", p.min_requests, 1, 100_000)?;
            Ok(Params::ErrorRate(ErrorRate {
                scope,
                subject,
                percent: percent as u8,
                window_minutes: window as u32,
                min_requests: min as u32,
            }))
        }
        "circuit_open" => {
            let p: CircuitParams = read(value)?;
            for (what, v) in [("provider", &p.provider), ("model", &p.model)] {
                if let Some(v) = v {
                    name_ok(what, v)?;
                }
            }
            Ok(Params::Circuit(p))
        }
        _ => Err("kind must be budget, error_rate or circuit_open".to_string()),
    }
}

impl Params {
    /// The parameters as stored: every default written out.
    pub fn to_value(&self) -> Value {
        match self {
            Params::Budget { budget_id, percent } => {
                json!({ "budget_id": budget_id, "percent": percent })
            }
            Params::ErrorRate(e) => json!({
                "scope": e.scope.as_str(),
                "subject": e.subject,
                "percent": e.percent,
                "window_minutes": e.window_minutes,
                "min_requests": e.min_requests,
            }),
            Params::Circuit(c) => json!({ "provider": c.provider, "model": c.model }),
        }
    }
}

/// The subject text of an error-rate episode.
pub fn rate_subject(scope: Scope, subject: &str) -> String {
    match scope {
        Scope::Gateway => "gateway".to_string(),
        _ => format!("{}:{subject}", scope.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_params_are_checked() {
        let ok = parse("budget", &json!({ "percent": 75 })).unwrap();
        assert_eq!(
            ok,
            Params::Budget {
                budget_id: None,
                percent: 75
            }
        );
        let ok = parse("budget", &json!({ "budget_id": 3, "percent": 100 })).unwrap();
        assert!(matches!(
            ok,
            Params::Budget {
                budget_id: Some(3),
                ..
            }
        ));
        for bad in [
            json!({}),
            json!({ "percent": 0 }),
            json!({ "percent": 101 }),
            json!({ "percent": "7" }),
            json!({ "percent": 5, "budget_id": 0 }),
            json!({ "percent": 5, "extra": 1 }),
            json!(null),
            json!([]),
        ] {
            assert!(parse("budget", &bad).is_err(), "{bad}");
        }
        assert!(parse("budget", &json!({ "percent": 5, "extra": 1 }))
            .unwrap_err()
            .contains("unknown field"));
    }

    #[test]
    fn error_rate_params_take_defaults_and_refuse_bad_values() {
        let p = parse("error_rate", &json!({ "scope": "route", "percent": 10 })).unwrap();
        let Params::ErrorRate(e) = &p else { panic!() };
        assert_eq!(
            (e.window_minutes, e.min_requests, e.subject.clone()),
            (5, 20, None)
        );
        assert_eq!(
            p.to_value(),
            json!({ "scope": "route", "subject": null, "percent": 10, "window_minutes": 5, "min_requests": 20 })
        );
        assert!(parse("error_rate", &json!({ "scope": "gateway", "percent": 1, "window_minutes": 60, "min_requests": 100000 })).is_ok());
        for bad in [
            json!({ "scope": "team", "percent": 10 }),
            json!({ "scope": "route", "percent": 0 }),
            json!({ "scope": "route", "percent": 10, "window_minutes": 4 }),
            json!({ "scope": "route", "percent": 10, "window_minutes": 61 }),
            json!({ "scope": "route", "percent": 10, "min_requests": 0 }),
            json!({ "scope": "route", "percent": 10, "min_requests": 100001 }),
            json!({ "scope": "gateway", "percent": 10, "subject": "x" }),
            json!({ "scope": "key", "percent": 10, "subject": "abc" }),
            json!({ "scope": "key", "percent": 10, "subject": "0" }),
            json!({ "scope": "route", "percent": 10, "subject": " chat" }),
            json!({ "scope": "route", "percent": 10, "subject": "" }),
            json!({ "scope": "route", "percent": 10, "bogus": true }),
            json!({ "percent": 10 }),
        ] {
            assert!(parse("error_rate", &bad).is_err(), "{bad}");
        }
        let ok = parse(
            "error_rate",
            &json!({ "scope": "key", "percent": 10, "subject": "12" }),
        );
        assert!(ok.is_ok());
    }

    #[test]
    fn circuit_params_and_unknown_kinds() {
        assert!(parse("circuit_open", &json!({})).is_ok());
        assert!(parse(
            "circuit_open",
            &json!({ "provider": "openai", "model": null })
        )
        .is_ok());
        assert!(parse("circuit_open", &json!({ "provider": "" })).is_err());
        assert!(parse("circuit_open", &json!({ "x": 1 })).is_err());
        assert!(parse("latency", &json!({})).is_err());
    }
}
