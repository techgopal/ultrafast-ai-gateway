//! Rule compilation and the shared matching/redaction engine used by both
//! whole-text checks and the stream scanner.

use std::borrow::Cow;
use std::sync::Arc;

use regex::{Error as RegexError, Regex, RegexBuilder};

use super::{
    keywords, pii, Action, Compiled, Direction, Directions, GuardrailError, Matcher, Outcome,
    PiiType, RuleSpec, MAX_RULES, REGEX_SIZE_LIMIT,
};

pub(crate) const PLAIN_PLACEHOLDER: &str = "[REDACTED]";

/// Which view of the text a pattern is matched against.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Src {
    /// The text as it is: regex rules, and keywords that contain a backslash
    /// or a character of a script written without spaces.
    Raw,
    /// JSON escapes and unspaced-script characters blanked, so they are
    /// boundaries: other keywords.
    Masked,
}

pub(crate) enum Finder {
    /// Keywords or a regex rule: counted under the rule id.
    Plain(Regex, Src),
    Pii(PiiType),
}

/// The views of one text a scan reads; all have the same byte length.
pub(crate) struct Hays<'a> {
    pub raw: &'a str,
    /// JSON escapes blanked; scripts kept (email addresses may use them).
    pub esc: &'a str,
    /// Escapes and unspaced-script characters blanked (with spaces).
    pub masked: &'a str,
    /// Escapes and unspaced-script characters filled with [`EDGE`]: a byte no
    /// detector reads as a separator or as part of a word (spaces and dashes
    /// join the digits of a phone number; this does not).
    pub pii: &'a str,
}

/// The fill of the view the PII detectors read.
pub(crate) const EDGE: u8 = 0x1f;

pub(crate) struct CompiledRule {
    pub id: String,
    pub action: Action,
    pub directions: Directions,
    pub finders: Vec<Finder>,
}

pub(crate) fn compile_rules(rules: &[RuleSpec]) -> Result<Vec<CompiledRule>, GuardrailError> {
    if rules.len() > MAX_RULES {
        return Err(GuardrailError::TooManyRules);
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(rules.len());
    for r in rules {
        if r.id.trim().is_empty() || !seen.insert(r.id.as_str()) {
            return Err(GuardrailError::BadRuleId(r.id.clone()));
        }
        let finders = match &r.matcher {
            Matcher::Keywords { words, whole_word } => keywords::build(&r.id, words, *whole_word)?
                .into_iter()
                .map(|(re, src)| Finder::Plain(re, src))
                .collect(),
            Matcher::Regex(p) => vec![Finder::Plain(build_regex(&r.id, p)?, Src::Raw)],
            Matcher::Pii(types) => {
                if types.is_empty() {
                    return Err(GuardrailError::NoPiiTypes(r.id.clone()));
                }
                let mut types = types.clone();
                types.sort();
                types.dedup();
                types.into_iter().map(Finder::Pii).collect()
            }
        };
        out.push(CompiledRule {
            id: r.id.clone(),
            action: r.action,
            directions: r.directions,
            finders,
        });
    }
    Ok(out)
}

fn build_regex(rule: &str, pattern: &str) -> Result<Regex, GuardrailError> {
    let re = RegexBuilder::new(pattern)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
        .map_err(|e| match e {
            RegexError::CompiledTooBig(_) => GuardrailError::RegexTooLarge(rule.to_string()),
            other => GuardrailError::InvalidRegex(rule.to_string(), other.to_string()),
        })?;
    if re.is_match("") {
        return Err(GuardrailError::RegexMatchesEmpty(rule.to_string()));
    }
    // `^`, `$`, `\A`, `\z` (and the multi-line forms) mean "start/end of the
    // text"; a stream only ever sees part of it, so they would behave
    // differently there than on a whole answer.
    let anchored = regex_syntax::Parser::new()
        .parse(pattern)
        .map(|hir| hir.properties().look_set().contains_anchor())
        .unwrap_or(false);
    if anchored {
        return Err(GuardrailError::RegexAnchor(rule.to_string()));
    }
    Ok(re)
}

/// Byte index of the character after the one at `i` (`i + 1` at the end).
pub(crate) fn next_char(hay: &str, i: usize) -> usize {
    hay[i..].chars().next().map_or(i + 1, |c| i + c.len_utf8())
}

fn find_plain(re: &Regex, hay: &str, from: usize, out: &mut Vec<pii::Span>) {
    let mut pos = from;
    while pos <= hay.len() {
        let Some(m) = re.find_at(hay, pos) else { break };
        if m.end() == m.start() {
            pos = next_char(hay, m.start());
        } else {
            out.push(pii::Span {
                start: m.start(),
                end: m.end(),
                matched: true,
                open: false,
            });
            pos = m.end();
        }
    }
}

/// Byte spans of JSON string escapes (`\n \t \r \" \\ \/ \b \f \uXXXX`) in
/// `raw`, scanning from `from` (which must be the start of a token).
pub(crate) fn escape_spans(raw: &str, from: usize, out: &mut Vec<(usize, usize)>) {
    let b = raw.as_bytes();
    let mut i = from;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 >= b.len() {
            i += 1;
            continue;
        }
        match b[i + 1] {
            b'n' | b't' | b'r' | b'"' | b'\\' | b'/' | b'b' | b'f' => {
                out.push((i, i + 2));
                i += 2;
            }
            b'u' if i + 6 <= b.len() && b[i + 2..i + 6].iter().all(u8::is_ascii_hexdigit) => {
                out.push((i, i + 6));
                i += 6;
            }
            _ => i += 1,
        }
    }
}

/// Characters of scripts written without spaces between words (Han, Hiragana,
/// Katakana, Thai, Lao, Khmer, Myanmar): Unicode word boundaries never fall
/// between two of them, so they are not treated as word characters (a
/// keyword containing one is matched as a substring).
pub(crate) fn is_unspaced_script(c: char) -> bool {
    matches!(u32::from(c),
        0x0E00..=0x0EFF // Thai, Lao
        | 0x1000..=0x109F // Myanmar
        | 0x1780..=0x17FF // Khmer
        | 0x3040..=0x30FF // Hiragana, Katakana
        | 0x31F0..=0x31FF // Katakana extensions
        | 0x3400..=0x4DBF // Han extension A
        | 0x4E00..=0x9FFF // Han
        | 0xF900..=0xFAFF // Han compatibility
        | 0xFF66..=0xFF9F // half-width Katakana
        | 0x20000..=0x3FFFF // Han extensions B and later
    )
}

/// `raw` with every JSON escape replaced by blanks of the same length, so
/// detectors see a boundary there (tool-call arguments are JSON text, and the
/// `n` of `\n` is not a letter of the next word). A `\uXXXX` escape of a
/// letter or digit becomes letters (`jos\u00e9@x.com` is one address).
/// Positions are unchanged.
pub(crate) fn mask_escapes(raw: &str) -> Cow<'_, str> {
    mask_escapes_with(raw, b' ')
}

/// [`mask_escapes`] with the blank byte of the caller's choice (ASCII).
pub(crate) fn mask_escapes_with(raw: &str, blank: u8) -> Cow<'_, str> {
    if !raw.contains('\\') {
        return Cow::Borrowed(raw);
    }
    let mut spans = Vec::new();
    escape_spans(raw, 0, &mut spans);
    if spans.is_empty() {
        return Cow::Borrowed(raw);
    }
    let mut bytes = raw.as_bytes().to_vec();
    for (s, e) in spans {
        let fill = if e - s == 6 {
            u32::from_str_radix(&raw[s + 2..e], 16)
                .ok()
                .and_then(char::from_u32)
                .filter(|c| c.is_alphanumeric() && !is_unspaced_script(*c))
                .map_or(blank, |_| b'x')
        } else {
            blank
        };
        bytes[s..e].fill(fill);
    }
    // only ASCII bytes were replaced by ASCII, so this is valid UTF-8
    Cow::Owned(String::from_utf8(bytes).unwrap_or_else(|_| raw.to_string()))
}

/// `text` with each character of an unspaced script replaced by as many
/// blanks as it has bytes (positions unchanged).
pub(crate) fn blank_scripts(text: &str) -> Cow<'_, str> {
    blank_scripts_with(text, b' ')
}

/// [`blank_scripts`] with the blank byte of the caller's choice (ASCII).
pub(crate) fn blank_scripts_with(text: &str, blank: u8) -> Cow<'_, str> {
    if !text.chars().any(is_unspaced_script) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if is_unspaced_script(c) {
            out.extend(std::iter::repeat_n(char::from(blank), c.len_utf8()));
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// One match of one rule.
pub(crate) struct Hit<'a> {
    pub start: usize,
    pub end: usize,
    /// Position of the finder in the set (guardrail, rule, finder order).
    pub seq: usize,
    /// `None` for a rejected candidate, kept only so a stream is not cut inside it.
    pub action: Option<Action>,
    pub guardrail: &'a Compiled,
    pub rule: &'a CompiledRule,
    /// Key of the redaction count: the PII type name or the rule id.
    pub label: &'a str,
    pub placeholder: &'a str,
    /// A private-key block still waiting for its END line.
    pub open: bool,
}

/// Every match at or after `from` of every rule that covers `dir`. With
/// `rejected`, candidates a detector turned down are listed too (a stream
/// needs them to cut safely; a whole-text check does not).
pub(crate) fn collect_hits<'a>(
    set: &'a [Arc<Compiled>],
    dir: Direction,
    hays: &Hays<'_>,
    from: usize,
    rejected: bool,
) -> Vec<Hit<'a>> {
    let mut hits = Vec::new();
    let mut spans = Vec::new();
    let mut seq = 0usize;
    for g in set {
        for rule in &g.rules {
            for finder in &rule.finders {
                seq += 1;
                if !rule.directions.covers(dir) {
                    continue;
                }
                spans.clear();
                let (label, placeholder) = match finder {
                    Finder::Plain(re, src) => {
                        let hay = if *src == Src::Raw {
                            hays.raw
                        } else {
                            hays.masked
                        };
                        find_plain(re, hay, from, &mut spans);
                        (rule.id.as_str(), PLAIN_PLACEHOLDER)
                    }
                    Finder::Pii(ty) => {
                        // addresses may use any script; the rest reads blanked scripts
                        let hay = if *ty == PiiType::Email {
                            hays.esc
                        } else {
                            hays.pii
                        };
                        pii::find_all(*ty, hay, from, rejected, &mut spans);
                        (ty.name(), ty.placeholder())
                    }
                };
                for sp in &spans {
                    hits.push(Hit {
                        start: sp.start,
                        end: sp.end,
                        seq,
                        action: sp.matched.then_some(rule.action),
                        guardrail: g,
                        rule,
                        label,
                        placeholder,
                        open: sp.open,
                    });
                }
            }
        }
    }
    hits
}

/// The first blocking hit in set order.
pub(crate) fn first_block<'h, 'a>(
    hits: impl IntoIterator<Item = &'h Hit<'a>>,
) -> Option<&'h Hit<'a>>
where
    'a: 'h,
{
    hits.into_iter()
        .filter(|h| h.action == Some(Action::Block))
        .min_by_key(|h| (h.seq, h.start))
}

/// One replacement: a run of overlapping redact matches.
pub(crate) struct Applied<'a> {
    pub start: usize,
    pub end: usize,
    /// Count key and placeholder of the match that starts first (ties: the
    /// earlier rule).
    pub label: &'a str,
    pub placeholder: &'a str,
    /// The run includes a private-key block that has no END line yet.
    pub open: bool,
}

/// Redactions to apply: overlapping redact matches merge into one span (their
/// union), so no part of a match stays visible and nothing is replaced twice.
pub(crate) fn resolve<'h, 'a>(hits: impl IntoIterator<Item = &'h Hit<'a>>) -> Vec<Applied<'a>>
where
    'a: 'h,
{
    let mut redact: Vec<&Hit<'a>> = hits
        .into_iter()
        .filter(|h| h.action == Some(Action::Redact))
        .collect();
    redact.sort_by_key(|h| (h.start, h.seq));
    let mut applied: Vec<Applied<'a>> = Vec::new();
    for h in redact {
        match applied.last_mut() {
            Some(last) if h.start < last.end => {
                last.end = last.end.max(h.end);
                last.open |= h.open;
            }
            _ => applied.push(Applied {
                start: h.start,
                end: h.end,
                label: h.label,
                placeholder: h.placeholder,
                open: h.open,
            }),
        }
    }
    applied
}

/// `hay[from..to]` with the applied spans (sorted, inside the range) replaced.
pub(crate) fn render<'x, 'a: 'x>(
    hay: &str,
    from: usize,
    to: usize,
    applied: impl IntoIterator<Item = &'x Applied<'a>>,
) -> String {
    let mut out = String::with_capacity(to - from);
    let mut at = from;
    for h in applied {
        out.push_str(&hay[at..h.start]);
        out.push_str(h.placeholder);
        at = h.end;
    }
    out.push_str(&hay[at..to]);
    out
}

pub(crate) fn blocked_of(h: &Hit<'_>) -> (i64, String) {
    (h.guardrail.id, h.guardrail.name.clone())
}

pub(crate) fn check_texts(set: &[Arc<Compiled>], dir: Direction, texts: &mut [String]) -> Outcome {
    let mut outcome = Outcome::default();
    let all: Vec<Vec<Hit<'_>>> = texts
        .iter()
        .map(|t| {
            let esc = mask_escapes(t);
            let masked = blank_scripts(&esc);
            let pii_esc = mask_escapes_with(t, EDGE);
            let pii_view = blank_scripts_with(&pii_esc, EDGE);
            let hays = Hays {
                raw: t,
                esc: &esc,
                masked: &masked,
                pii: &pii_view,
            };
            collect_hits(set, dir, &hays, 0, false)
        })
        .collect();
    for h in all
        .iter()
        .flatten()
        .filter(|h| h.action == Some(Action::Flag))
    {
        outcome.add_flag(h.guardrail.id, &h.rule.id);
    }
    if let Some(b) = first_block(all.iter().flatten()) {
        outcome.blocked_by = Some(blocked_of(b));
        return outcome;
    }
    for (text, hits) in texts.iter_mut().zip(&all) {
        let applied = resolve(hits);
        if applied.is_empty() {
            continue;
        }
        for h in &applied {
            outcome.add_redaction(h.label);
        }
        *text = render(text, 0, text.len(), &applied);
    }
    outcome
}
