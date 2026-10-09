//! Rule compilation and the shared matching/redaction engine used by both
//! whole-text checks and the stream scanner.

use std::borrow::Cow;
use std::cmp::Reverse;
use std::sync::Arc;

use regex::{Error as RegexError, Regex, RegexBuilder};

use super::{
    keywords, pii, Action, Compiled, Direction, Directions, GuardrailError, Matcher, Outcome,
    PiiType, RuleSpec, MAX_RULES, REGEX_SIZE_LIMIT,
};

pub(crate) const PLAIN_PLACEHOLDER: &str = "[REDACTED]";

pub(crate) enum Finder {
    /// Keywords or a regex rule: counted under the rule id.
    Plain(Regex),
    Pii(PiiType),
}

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
            Matcher::Keywords { words, whole_word } => {
                vec![Finder::Plain(keywords::build(&r.id, words, *whole_word)?)]
            }
            Matcher::Regex(p) => vec![Finder::Plain(build_regex(&r.id, p)?)],
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

/// `raw` with every JSON escape replaced by spaces of the same length, so
/// detectors see a boundary there (tool-call arguments are JSON text, and the
/// `n` of `\n` is not a letter of the next word). Positions are unchanged.
pub(crate) fn mask_escapes(raw: &str) -> Cow<'_, str> {
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
        bytes[s..e].fill(b' ');
    }
    // only ASCII bytes were replaced by ASCII, so this is valid UTF-8
    Cow::Owned(String::from_utf8(bytes).unwrap_or_else(|_| raw.to_string()))
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

/// Every match at or after `from` of every rule that covers `dir`.
pub(crate) fn collect_hits<'a>(
    set: &'a [Arc<Compiled>],
    dir: Direction,
    hay: &str,
    from: usize,
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
                    Finder::Plain(re) => {
                        find_plain(re, hay, from, &mut spans);
                        (rule.id.as_str(), PLAIN_PLACEHOLDER)
                    }
                    Finder::Pii(ty) => {
                        pii::find_all(*ty, hay, from, &mut spans);
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

/// Redact hits to apply: leftmost first, then longest, then the earlier rule;
/// nothing overlapping an earlier choice.
pub(crate) fn resolve<'h, 'a>(hits: impl IntoIterator<Item = &'h Hit<'a>>) -> Vec<&'h Hit<'a>>
where
    'a: 'h,
{
    let mut redact: Vec<&Hit<'a>> = hits
        .into_iter()
        .filter(|h| h.action == Some(Action::Redact))
        .collect();
    redact.sort_by_key(|h| (h.start, Reverse(h.end), h.seq));
    let mut applied = Vec::new();
    let mut last_end = 0usize;
    for h in redact {
        if applied.is_empty() || h.start >= last_end {
            last_end = h.end;
            applied.push(h);
        }
    }
    applied
}

/// `hay[from..to]` with the applied hits (sorted, inside the range) replaced.
pub(crate) fn render(hay: &str, from: usize, to: usize, applied: &[&Hit<'_>]) -> String {
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
        .map(|t| collect_hits(set, dir, &mask_escapes(t), 0))
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
