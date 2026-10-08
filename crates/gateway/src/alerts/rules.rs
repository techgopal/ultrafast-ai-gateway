//! What a rule is about: the parameters of each kind, checked.
//!
//! A rule's `params` are JSON, by kind: `budget` `{budget_id: id|null,
//! percent: 1-100}`; `error_rate` `{scope, subject, percent, window_minutes
//! 5-60 (5), min_requests 1-100000 (20)}`; `circuit_open` `{provider, model}`.
//! They are checked when the rule is written (unknown fields are refused) and again when the engine loads them.

use serde_json::{json, Value};

use super::errors_window::Scope;

/// The kinds of rule, as stored.
pub const KINDS: [&str; 3] = ["budget", "error_rate", "circuit_open"];

pub const DEFAULT_WINDOW_MINUTES: i64 = 5;
pub const DEFAULT_MIN_REQUESTS: i64 = 20;
/// The longest a name in the parameters may be.
const MAX_NAME: usize = 200;

/// `circuit_open`: fires when a breaker opens, resolves when it closes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircuitParams {
    /// `null` or left out: any provider.
    pub provider: Option<String>,
    /// `null` or left out: any model.
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

/// What is wrong with the parameters: the field to show it on
/// (`params`, or `params.<name>`) and a message that never repeats a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamError {
    pub field: String,
    pub message: String,
}

impl ParamError {
    fn on(name: &str, message: impl Into<String>) -> Self {
        Self {
            field: format!("params.{name}"),
            message: message.into(),
        }
    }

    fn whole(message: impl Into<String>) -> Self {
        Self {
            field: "params".to_string(),
            message: message.into(),
        }
    }
}

type Fields = serde_json::Map<String, Value>;

fn object<'a>(value: &'a Value, known: &[&str]) -> Result<&'a Fields, ParamError> {
    let map = value
        .as_object()
        .ok_or_else(|| ParamError::whole("params must be an object"))?;
    // Names are the caller's: they are named in the field key, not repeated
    // in the message.
    if let Some(unknown) = map.keys().find(|k| !known.contains(&k.as_str())) {
        let shown: String = unknown.chars().take(60).collect();
        return Err(ParamError::on(&shown, "unknown field"));
    }
    Ok(map)
}

fn integer(map: &Fields, name: &str, default: Option<i64>) -> Result<i64, ParamError> {
    match map.get(name) {
        None => default.ok_or_else(|| ParamError::on(name, "is required")),
        Some(v) => v
            .as_i64()
            .ok_or_else(|| ParamError::on(name, "must be a whole number")),
    }
}

fn ranged(
    map: &Fields,
    name: &str,
    default: Option<i64>,
    low: i64,
    high: i64,
) -> Result<i64, ParamError> {
    let value = integer(map, name, default)?;
    if (low..=high).contains(&value) {
        Ok(value)
    } else {
        Err(ParamError::on(
            name,
            format!("must be from {low} to {high}"),
        ))
    }
}

/// A text or null.
fn optional_text<'a>(map: &'a Fields, name: &str) -> Result<Option<&'a str>, ParamError> {
    match map.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(ParamError::on(name, "must be text or null")),
    }
}

fn name_ok(what: &str, value: &str) -> Result<(), ParamError> {
    if value.trim().is_empty() || value.trim() != value {
        return Err(ParamError::on(
            what,
            "must not be empty or start or end with a space",
        ));
    }
    if value.len() > MAX_NAME || value.chars().any(char::is_control) {
        return Err(ParamError::on(
            what,
            format!("must be at most {MAX_NAME} bytes, without control characters"),
        ));
    }
    Ok(())
}

pub fn parse(kind: &str, value: &Value) -> Result<Params, ParamError> {
    match kind {
        "budget" => {
            let map = object(value, &["budget_id", "percent"])?;
            let budget_id = match map.get("budget_id") {
                None | Some(Value::Null) => None,
                Some(v) => match v.as_i64() {
                    Some(id) if id >= 1 => Some(id),
                    _ => return Err(ParamError::on("budget_id", "must be a budget id or null")),
                },
            };
            let percent = ranged(map, "percent", None, 1, 100)?;
            Ok(Params::Budget {
                budget_id,
                percent: percent as u8,
            })
        }
        "error_rate" => {
            let map = object(
                value,
                &[
                    "scope",
                    "subject",
                    "percent",
                    "window_minutes",
                    "min_requests",
                ],
            )?;
            let scope = optional_text(map, "scope")?
                .ok_or_else(|| ParamError::on("scope", "is required"))?;
            let scope = Scope::parse(scope).ok_or_else(|| {
                ParamError::on("scope", "must be gateway, route, provider or key")
            })?;
            let subject = match (optional_text(map, "subject")?, scope) {
                (None, _) => None,
                (Some(_), Scope::Gateway) => {
                    return Err(ParamError::on(
                        "subject",
                        "is not used for the scope gateway",
                    ))
                }
                (Some(s), Scope::Key) => {
                    if s.parse::<i64>().map_or(true, |id| id < 1) || s.starts_with('+') {
                        return Err(ParamError::on("subject", "must be a key id"));
                    }
                    Some(s.to_string())
                }
                (Some(s), _) => {
                    name_ok("subject", s)?;
                    Some(s.to_string())
                }
            };
            let percent = ranged(map, "percent", None, 1, 100)?;
            let window = ranged(map, "window_minutes", Some(DEFAULT_WINDOW_MINUTES), 5, 60)?;
            let min = ranged(map, "min_requests", Some(DEFAULT_MIN_REQUESTS), 1, 100_000)?;
            Ok(Params::ErrorRate(ErrorRate {
                scope,
                subject,
                percent: percent as u8,
                window_minutes: window as u32,
                min_requests: min as u32,
            }))
        }
        "circuit_open" => {
            let map = object(value, &["provider", "model"])?;
            let mut out = CircuitParams {
                provider: None,
                model: None,
            };
            for (what, slot) in [("provider", &mut out.provider), ("model", &mut out.model)] {
                if let Some(v) = optional_text(map, what)? {
                    name_ok(what, v)?;
                    *slot = Some(v.to_string());
                }
            }
            Ok(Params::Circuit(out))
        }
        _ => Err(ParamError::whole(
            "kind must be budget, error_rate or circuit_open",
        )),
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
        let e = parse("budget", &json!({ "percent": 5, "extra": 1 })).unwrap_err();
        assert_eq!(
            (e.field.as_str(), e.message.as_str()),
            ("params.extra", "unknown field")
        );
        let e = parse("budget", &json!({ "percent": 500 })).unwrap_err();
        assert_eq!(e.field, "params.percent");
        let e = parse("budget", &json!({})).unwrap_err();
        assert_eq!(
            (e.field.as_str(), e.message.as_str()),
            ("params.percent", "is required")
        );
        let e = parse("budget", &json!([])).unwrap_err();
        assert_eq!(e.field, "params");
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
