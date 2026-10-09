//! Prompt templates: versions of messages with `{{variables}}`, rendered into
//! a call before anything else looks at it.
//!
//! The rules, which the admin API and `/v1` share:
//! - A variable is `{{name}}` with a name of `[A-Za-z_][A-Za-z0-9_]{0,63}`,
//!   nothing between the braces and the name. Anything else with braces is
//!   text (`{{ name }}`, `{{1x}}`, `{{}}`), and is neither asked for nor
//!   replaced.
//! - The variables of a version are the names its messages use. A call gives
//!   a value for every one of them (missing: refused) and for no other
//!   (unknown: refused). Names are case sensitive.
//! - A value is a string of at most 32 KiB. It is put in as it is: nothing is
//!   escaped, and it is not read again, so a value that holds `{{other}}`
//!   stays that text.
//! - The rendered messages are at most 1 MiB together.
//! - The template\'s messages come first, then the call\'s own. The template\'s
//!   model is used when the call names none; the call\'s own `temperature`,
//!   `max_tokens`, `top_p` and `response_format` win over the template\'s.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ultrafast_translate::ingress::openai::parse_response_format;
use ultrafast_translate::ingress::prompt::PromptRef;
use ultrafast_translate::types::{ChatRequest, Message, ResponseFormat, Role};

use crate::store::{TemplateRow, VersionRow};

/// The longest value of a variable, in bytes.
pub const MAX_VALUE_BYTES: usize = 32 * 1024;
/// The most the rendered messages of a call may be, in bytes.
pub const MAX_RENDERED_BYTES: usize = 1024 * 1024;
/// Most messages in a version.
pub const MAX_MESSAGES: usize = 64;
/// Longest message of a version, in bytes.
pub const MAX_CONTENT_BYTES: usize = 64 * 1024;
/// The most the messages of a version may be together, in bytes.
pub const MAX_TOTAL_BYTES: usize = 256 * 1024;
/// Most variables in a version.
pub const MAX_VARIABLES: usize = 64;
/// Most versions of a template.
pub const MAX_VERSIONS: usize = 200;
/// Most templates.
pub const MAX_TEMPLATES: usize = 1000;
/// How many older versions are kept in memory, to be read from the
/// database only once.
pub const OLD_VERSIONS_KEPT: usize = 256;
/// The longest a model name of a template is, in characters.
pub const MAX_MODEL_CHARS: usize = 200;
/// The roles a template message may have.
pub const ROLES: [&str; 4] = ["system", "developer", "user", "assistant"];

/// A message of a version, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateMessage {
    /// `system`, `developer`, `user` or `assistant`.
    pub role: String,
    /// Text, with `{{name}}` where a value goes.
    pub content: String,
}

/// The settings a version carries for the call. All optional; what the call
/// itself sets wins.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Params {
    /// 0 to 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// At least 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// 0 to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// As in `/v1/chat/completions`: `{"type":"text"}`, `{"type":"json_object"}`
    /// or `{"type":"json_schema","json_schema":{...}}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub response_format: Option<Value>,
}

/// Whether `s` can be the name of a variable.
pub fn is_variable_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_alphabetic() || b[0] == b'_')
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

/// One piece of a message: text, or a variable.
enum Piece<'a> {
    Text(&'a str),
    Var(&'a str),
}

/// Cuts a message into text and `{{name}}` variables. Only ASCII is cut at,
/// so every piece is valid text.
fn pieces(text: &str) -> Vec<Piece<'_>> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut from) = (0, 0);
    while i + 1 < b.len() {
        if b[i] == b'{' && b[i + 1] == b'{' {
            let start = i + 2;
            let mut j = start;
            while j < b.len() && j - start <= 64 && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            let name = &text[start..j];
            if is_variable_name(name) && b.get(j) == Some(&b'}') && b.get(j + 1) == Some(&b'}') {
                if from < i {
                    out.push(Piece::Text(&text[from..i]));
                }
                out.push(Piece::Var(name));
                i = j + 2;
                from = i;
                continue;
            }
        }
        i += 1;
    }
    if from < b.len() {
        out.push(Piece::Text(&text[from..]));
    }
    out
}

/// The names of the variables a text uses, once each, sorted.
pub fn variables_in<'a>(texts: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut names = BTreeSet::new();
    for text in texts {
        for piece in pieces(text) {
            if let Piece::Var(name) = piece {
                names.insert(name.to_string());
            }
        }
    }
    names.into_iter().collect()
}

/// Why a template could not be rendered. The messages name variables and
/// nothing else the caller sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    Unknown(Vec<String>),
    Missing(Vec<String>),
    TooLong(String),
    TooLarge,
}

/// A name for a message: cut and free of control characters, as it came
/// from the caller.
fn shown(name: &str) -> String {
    let mut out: String = name
        .chars()
        .take(64)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    if name.chars().count() > 64 {
        out.push('…');
    }
    out
}

fn listed(names: &[String]) -> String {
    let mut parts: Vec<String> = names
        .iter()
        .take(5)
        .map(|n| format!("'{}'", shown(n)))
        .collect();
    if names.len() > 5 {
        parts.push("…".to_string());
    }
    parts.join(", ")
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plural = |n: usize| if n == 1 { "variable" } else { "variables" };
        match self {
            Self::Unknown(n) => write!(f, "unknown {} {}", plural(n.len()), listed(n)),
            Self::Missing(n) => write!(f, "missing {} {}", plural(n.len()), listed(n)),
            Self::TooLong(n) => write!(
                f,
                "variable '{}' is longer than {MAX_VALUE_BYTES} bytes",
                shown(n)
            ),
            Self::TooLarge => write!(
                f,
                "the rendered prompt is larger than {MAX_RENDERED_BYTES} bytes"
            ),
        }
    }
}

/// One version of a template, ready to render.
#[derive(Debug)]
pub struct Version {
    pub number: i64,
    pub messages: Vec<TemplateMessage>,
    /// Sorted.
    pub variables: Vec<String>,
    pub model: Option<String>,
    pub params: Params,
    /// `params.response_format`, read.
    pub response_format: Option<ResponseFormat>,
}

impl Version {
    /// Reads a stored version. A row that cannot be read is `None`: it is
    /// left out of the snapshot (and logged by the caller).
    pub fn of_row(row: &VersionRow) -> Option<Self> {
        let messages: Vec<TemplateMessage> = serde_json::from_str(&row.messages).ok()?;
        let params: Params = serde_json::from_str(&row.params).ok()?;
        let response_format = match &params.response_format {
            None => None,
            Some(v) => Some(parse_response_format(v).ok()?),
        };
        let variables = variables_in(messages.iter().map(|m| m.content.as_str()));
        Some(Self {
            number: row.version,
            messages,
            variables,
            model: row.model.clone(),
            params,
            response_format,
        })
    }

    /// The messages with the values put in, as call messages.
    pub fn render(&self, values: &BTreeMap<String, String>) -> Result<Vec<Message>, RenderError> {
        Ok(self
            .render_messages(values)?
            .into_iter()
            .map(|m| {
                let role = match m.role.as_str() {
                    "assistant" => Role::Assistant,
                    "user" => Role::User,
                    _ => Role::System,
                };
                Message::text(role, m.content)
            })
            .collect())
    }

    /// The messages with the values put in, with the roles as written.
    pub fn render_messages(
        &self,
        values: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateMessage>, RenderError> {
        let unknown: Vec<String> = values
            .keys()
            .filter(|k| !self.variables.contains(k))
            .cloned()
            .collect();
        if !unknown.is_empty() {
            return Err(RenderError::Unknown(unknown));
        }
        let missing: Vec<String> = self
            .variables
            .iter()
            .filter(|v| !values.contains_key(*v))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(RenderError::Missing(missing));
        }
        if let Some((name, _)) = values.iter().find(|(_, v)| v.len() > MAX_VALUE_BYTES) {
            return Err(RenderError::TooLong(name.clone()));
        }
        let mut total = 0usize;
        let mut out = Vec::with_capacity(self.messages.len());
        for m in &self.messages {
            let mut text = String::with_capacity(m.content.len());
            for piece in pieces(&m.content) {
                let part = match piece {
                    Piece::Text(t) => t,
                    Piece::Var(name) => values.get(name).map_or("", String::as_str),
                };
                total += part.len();
                if total > MAX_RENDERED_BYTES {
                    return Err(RenderError::TooLarge);
                }
                text.push_str(part);
            }
            out.push(TemplateMessage {
                role: m.role.clone(),
                content: text,
            });
        }
        Ok(out)
    }
}

/// A template as the snapshot holds it: the newest version only. Older
/// versions are immutable and read from the database when a call names one
/// (see [`OldVersions`]), so the snapshot, and what a refresh reads, stays
/// the size of one version per template.
#[derive(Debug)]
pub struct Template {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub latest: Arc<Version>,
}

impl Template {
    pub fn new(row: &TemplateRow, latest: Arc<Version>) -> Self {
        Self {
            id: row.id,
            name: row.name.clone(),
            created_at: row.created_at.clone(),
            latest,
        }
    }

    /// The number of the version a call gets: the latest, or the one asked
    /// for (digits), which must exist so far.
    pub fn wanted(&self, version: Option<&str>) -> Result<i64, UseError> {
        let Some(text) = version else {
            return Ok(self.latest.number);
        };
        let number: i64 = text
            .parse()
            .ok()
            .filter(|n| *n > 0)
            .ok_or(UseError::BadVersion)?;
        if number > self.latest.number {
            return Err(UseError::VersionNotFound(number));
        }
        Ok(number)
    }
}

/// Older versions read from the database, the last [`OLD_VERSIONS_KEPT`]
/// used. A version never changes, so an entry is right for as long as its
/// template exists: a refresh drops the entries of a template that is gone
/// or was made again (another creation time).
#[derive(Default)]
pub struct OldVersions {
    inner: std::sync::Mutex<OldInner>,
}

#[derive(Default)]
struct OldInner {
    tick: u64,
    /// (template id, created_at, version) -> (last used, version)
    entries: BTreeMap<(i64, String, i64), (u64, Arc<Version>)>,
}

impl OldVersions {
    fn lock(&self) -> std::sync::MutexGuard<'_, OldInner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn get(&self, template: &Template, number: i64) -> Option<Arc<Version>> {
        let mut inner = self.lock();
        inner.tick += 1;
        let tick = inner.tick;
        let entry = inner
            .entries
            .get_mut(&(template.id, template.created_at.clone(), number))?;
        entry.0 = tick;
        Some(entry.1.clone())
    }

    pub fn put(&self, template: &Template, version: Arc<Version>) {
        let mut inner = self.lock();
        inner.tick += 1;
        let tick = inner.tick;
        let key = (template.id, template.created_at.clone(), version.number);
        inner.entries.insert(key, (tick, version));
        while inner.entries.len() > OLD_VERSIONS_KEPT {
            let oldest = inner
                .entries
                .iter()
                .min_by_key(|(_, (used, _))| *used)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => inner.entries.remove(&k),
                None => break,
            };
        }
    }

    /// Keeps the entries of templates that `is_current` still knows.
    pub fn retain(&self, is_current: impl Fn(i64, &str) -> bool) {
        self.lock()
            .entries
            .retain(|(id, created, _), _| is_current(*id, created));
    }

    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Why a call could not use its template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UseError {
    /// No template of that name.
    NotFound(String),
    VersionNotFound(i64),
    BadVersion,
    /// The database could not be read for an older version.
    Unavailable,
    Render(RenderError),
    /// The call and the template both name no model.
    NoModel,
}

impl UseError {
    /// Whether it is "no such thing" rather than a fault of the request.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound(_) | Self::VersionNotFound(_))
    }
}

impl fmt::Display for UseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "prompt template '{}' was not found", shown(name)),
            Self::VersionNotFound(n) => {
                write!(f, "version {n} of the prompt template was not found")
            }
            Self::BadVersion => write!(
                f,
                "prompt 'version' must be a positive integer, as a number or a string of digits"
            ),
            Self::Unavailable => write!(f, "the prompt version could not be read; try again"),
            Self::Render(e) => e.fmt(f),
            Self::NoModel => write!(
                f,
                "model is required: the request and the prompt template name none"
            ),
        }
    }
}

/// Puts a template into a call: its messages in front of the call\'s, its
/// model when the call has none, its settings where the call sets none.
pub fn apply(
    version: &Version,
    reference: &PromptRef,
    request: &mut ChatRequest,
) -> Result<(), UseError> {
    let mut messages = version
        .render(&reference.variables)
        .map_err(UseError::Render)?;
    if request.model.is_empty() {
        request.model = version.model.clone().ok_or(UseError::NoModel)?;
    }
    messages.append(&mut request.messages);
    request.messages = messages;
    if request.temperature.is_none() {
        request.temperature = version.params.temperature.map(|t| t as f32);
    }
    if request.top_p.is_none() {
        request.top_p = version.params.top_p.map(|t| t as f32);
    }
    if request.max_tokens.is_none() {
        request.max_tokens = version.params.max_tokens;
    }
    if request.response_format.is_none() {
        request.response_format = version.response_format.clone();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(texts: &[&str]) -> Version {
        Version {
            number: 1,
            messages: texts
                .iter()
                .map(|t| TemplateMessage {
                    role: "user".into(),
                    content: (*t).into(),
                })
                .collect(),
            variables: variables_in(texts.iter().copied()),
            model: None,
            params: Params::default(),
            response_format: None,
        }
    }

    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn text(v: &Version, pairs: &[(&str, &str)]) -> String {
        v.render(&values(pairs)).unwrap()[0].joined_text()
    }

    #[test]
    fn names_follow_the_pattern() {
        for ok in ["a", "_", "A1_", &"a".repeat(64)] {
            assert!(is_variable_name(ok), "{ok}");
        }
        for bad in ["", "1a", "a-b", "a b", "é", &"a".repeat(65)] {
            assert!(!is_variable_name(bad), "{bad}");
        }
    }

    #[test]
    fn only_exact_braces_make_a_variable() {
        let v = version(&["{{a}} {{ b }} {{}} {{1}} {a} {{{c}}} {{d} {{e}}"]);
        assert_eq!(v.variables, ["a", "c", "e"]);
        assert_eq!(
            text(&v, &[("a", "1"), ("c", "3"), ("e", "5")]),
            "1 {{ b }} {{}} {{1}} {a} {3} {{d} 5"
        );
    }

    #[test]
    fn a_value_is_put_in_once_and_as_it_is() {
        let v = version(&["{{a}}|{{b}}"]);
        assert_eq!(text(&v, &[("a", "{{b}}"), ("b", "{{a}}")]), "{{b}}|{{a}}");
        assert_eq!(text(&v, &[("a", "ü\n\"\\"), ("b", "")]), "ü\n\"\\|");
    }

    #[test]
    fn errors_name_the_variables_and_cut_what_the_caller_sent() {
        let v = version(&["{{a}} {{b}}"]);
        let e = v.render(&values(&[("a", "x")])).unwrap_err();
        assert_eq!(e.to_string(), "missing variable 'b'");
        let e = v.render(&values(&[])).unwrap_err();
        assert_eq!(e.to_string(), "missing variables 'a', 'b'");
        let long = "z".repeat(100);
        let e = v
            .render(&values(&[
                ("a", "x"),
                ("b", "y"),
                (&long, "1"),
                ("q\nr", "2"),
            ]))
            .unwrap_err();
        let shown = e.to_string();
        assert!(
            shown.starts_with("unknown variables 'q?r', 'zzzz"),
            "{shown}"
        );
        assert!(shown.len() < 200, "{shown}");
        let e = version(&["x"])
            .render(&values(&[("k", &"v".repeat(40_000))]))
            .unwrap_err();
        assert!(e.to_string().starts_with("unknown variable"));
    }

    #[test]
    fn what_is_rendered_is_bounded() {
        let v = version(&[&"{{a}}".repeat(100)]);
        let big = "x".repeat(MAX_VALUE_BYTES);
        assert_eq!(
            v.render(&values(&[("a", &big)])),
            Err(RenderError::TooLarge)
        );
        assert!(v.render(&values(&[("a", &big[..10_000])])).is_ok());
        assert_eq!(
            v.render(&values(&[("a", &"x".repeat(MAX_VALUE_BYTES + 1))])),
            Err(RenderError::TooLong("a".into()))
        );
    }

    #[test]
    fn a_template_goes_in_front_and_the_call_decides_what_it_sets() {
        let mut v = version(&["hi {{a}}"]);
        v.model = Some("p/m".into());
        v.params = Params {
            temperature: Some(0.5),
            max_tokens: Some(9),
            top_p: Some(0.5),
            response_format: None,
        };
        v.response_format = Some(ResponseFormat::JsonObject);
        let reference = PromptRef {
            id: "t".into(),
            version: None,
            variables: values(&[("a", "there")]),
        };
        let mut r = ChatRequest {
            model: String::new(),
            messages: vec![Message::text(Role::User, "more")],
            max_tokens: None,
            temperature: Some(1.0),
            top_p: None,
            stop: None,
            stream: false,
            tools: vec![],
            tool_choice: None,
            parallel_tool_calls: None,
            response_format: None,
            reasoning_effort: None,
        };
        apply(&v, &reference, &mut r).unwrap();
        assert_eq!(r.model, "p/m");
        assert_eq!(
            r.messages
                .iter()
                .map(Message::joined_text)
                .collect::<Vec<_>>(),
            ["hi there", "more"]
        );
        assert_eq!(
            (r.temperature, r.top_p, r.max_tokens),
            (Some(1.0), Some(0.5), Some(9))
        );
        assert_eq!(r.response_format, Some(ResponseFormat::JsonObject));
        // The call\'s own model stays; a template with none leaves a call
        // without one refused.
        let mut r2 = r.clone();
        r2.model = "q/n".into();
        r2.messages.clear();
        apply(&v, &reference, &mut r2).unwrap();
        assert_eq!(r2.model, "q/n");
        v.model = None;
        let mut r3 = r2.clone();
        r3.model.clear();
        assert_eq!(apply(&v, &reference, &mut r3), Err(UseError::NoModel));
    }

    fn template(latest: i64) -> Template {
        Template {
            id: 1,
            name: "t".into(),
            created_at: "2999-01-01 00:00:00".into(),
            latest: Arc::new(Version {
                number: latest,
                ..version(&["x"])
            }),
        }
    }

    #[test]
    fn versions_are_picked_by_digits() {
        let t = template(3);
        assert_eq!(t.wanted(None).unwrap(), 3);
        assert_eq!(t.wanted(Some("2")).unwrap(), 2);
        assert_eq!(t.wanted(Some("002")).unwrap(), 2);
        assert_eq!(
            t.wanted(Some("4")).unwrap_err(),
            UseError::VersionNotFound(4)
        );
        for bad in ["0", "", "x", "-1", "1.0", "99999999999999999999"] {
            assert_eq!(
                t.wanted(Some(bad)).unwrap_err(),
                UseError::BadVersion,
                "{bad}"
            );
        }
    }

    #[test]
    fn old_versions_are_kept_up_to_a_limit_and_dropped_with_their_template() {
        let cache = OldVersions::default();
        let t = template(1000);
        for n in 1..=(OLD_VERSIONS_KEPT as i64 + 10) {
            cache.put(
                &t,
                Arc::new(Version {
                    number: n,
                    ..version(&["x"])
                }),
            );
            // Keep version 1 in use: it must outlive the ones used once.
            cache.get(&t, 1);
        }
        assert_eq!(cache.len(), OLD_VERSIONS_KEPT);
        assert!(cache.get(&t, 1).is_some(), "the one in use stays");
        assert!(cache.get(&t, 2).is_none(), "the longest unused goes");
        let other = Template {
            created_at: "2999-01-02 00:00:00".into(),
            ..template(5)
        };
        assert!(
            cache.get(&other, 1).is_none(),
            "another creation is another template"
        );
        cache.retain(|id, created| id == 1 && created == "2999-01-02 00:00:00");
        assert!(cache.is_empty());
    }
}
