use regex::{Error as RegexError, Regex, RegexBuilder};

use super::{GuardrailError, MAX_KEYWORDS, MAX_KEYWORD_CHARS};

/// Compiled-size limit of a keyword alternation (up to 1 000 keywords).
const KEYWORD_SIZE_LIMIT: usize = 16 << 20;

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// One case-insensitive alternation over the escaped keywords, longest first.
/// Whole-word mode adds Unicode word boundaries on each side where the
/// keyword's edge character is a word character (a keyword like `c++` has no
/// boundary after the last `+`).
pub(crate) fn build(
    rule: &str,
    words: &[String],
    whole_word: bool,
) -> Result<Regex, GuardrailError> {
    if words.is_empty() {
        return Err(GuardrailError::NoKeywords(rule.to_string()));
    }
    if words.len() > MAX_KEYWORDS {
        return Err(GuardrailError::TooManyKeywords(rule.to_string()));
    }
    let mut seen = std::collections::HashSet::new();
    let mut list: Vec<&str> = Vec::with_capacity(words.len());
    for w in words {
        let w = w.trim();
        if w.is_empty() {
            return Err(GuardrailError::EmptyKeyword(rule.to_string()));
        }
        if w.chars().count() > MAX_KEYWORD_CHARS {
            return Err(GuardrailError::KeywordTooLong(rule.to_string()));
        }
        if seen.insert(w.to_lowercase()) {
            list.push(w);
        }
    }
    list.sort_by_key(|w| std::cmp::Reverse(w.chars().count()));
    let alternatives: Vec<String> = list
        .iter()
        .map(|w| {
            let mut s = String::new();
            if whole_word && w.chars().next().is_some_and(is_word) {
                s.push_str(r"\b");
            }
            s.push_str(&regex::escape(w));
            if whole_word && w.chars().next_back().is_some_and(is_word) {
                s.push_str(r"\b");
            }
            s
        })
        .collect();
    let pattern = format!("(?:{})", alternatives.join("|"));
    RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .size_limit(KEYWORD_SIZE_LIMIT)
        .build()
        .map_err(|e: RegexError| {
            let _ = e;
            GuardrailError::KeywordsTooLarge(rule.to_string())
        })
}
