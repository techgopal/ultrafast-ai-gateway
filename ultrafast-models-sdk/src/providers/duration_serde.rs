use std::time::Duration;
use serde::{Deserializer, Serializer};

pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let secs = duration.as_secs();
    let _nanos = duration.subsec_nanos();
    serializer.serialize_str(&format!("{}s", secs))
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    use serde_json::Value;

    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(s) => {
            parse_duration_string(&s).map_err(Error::custom)
        }
        Value::Object(obj) => {
            let secs = obj.get("secs")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| Error::custom("missing 'secs' field"))?;
            let nanos = obj.get("nanos")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            Ok(Duration::new(secs, nanos as u32))
        }
        _ => Err(Error::custom("invalid duration format")),
    }
}

fn parse_duration_string(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty duration string".to_string());
    }
    if let Some(stripped) = s.strip_suffix("ms") {
        let num: u64 = stripped.parse().map_err(|_| format!("invalid number: {}", stripped))?;
        return Ok(Duration::from_millis(num));
    }
    if let Some(stripped) = s.strip_suffix('s') {
        if !stripped.ends_with('m') && !stripped.ends_with('h') {
            let num: f64 = stripped.parse().map_err(|_| format!("invalid number: {}", stripped))?;
            let secs = num.trunc() as u64;
            let nanos = ((num.fract() * 1_000_000_000.0).round()) as u32;
            return Ok(Duration::new(secs, nanos));
        }
    }
    if let Some(stripped) = s.strip_suffix('m') {
        let num: u64 = stripped.parse().map_err(|_| format!("invalid number: {}", stripped))?;
        return Ok(Duration::from_secs(num * 60));
    }
    if let Some(stripped) = s.strip_suffix('h') {
        let num: u64 = stripped.parse().map_err(|_| format!("invalid number: {}", stripped))?;
        return Ok(Duration::from_secs(num * 3600));
    }
    Err(format!("unknown duration unit: {}", s))
}