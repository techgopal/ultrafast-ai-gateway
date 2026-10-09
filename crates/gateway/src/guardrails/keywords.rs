use regex::{Error as RegexError, Regex, RegexBuilder};

use super::{GuardrailError, MAX_KEYWORDS, MAX_KEYWORD_CHARS};

/// Compiled-size limit of a keyword alternation (up to 1 000 keywords).
const KEYWORD_SIZE_LIMIT: usize = 16 << 20;

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Characters of scripts written without spaces between words (Han, Hiragana,
/// Katakana, Thai, Lao, Khmer, Myanmar): Unicode word boundaries never fall
/// between two of them, so a keyword containing one is matched as a substring.
fn is_unspaced_script(c: char) -> bool {
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
            let bounded = whole_word && !w.chars().any(is_unspaced_script);
            if bounded && w.chars().next().is_some_and(is_word) {
                s.push_str(r"\b");
            }
            s.push_str(&regex::escape(w));
            if bounded && w.chars().next_back().is_some_and(is_word) {
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
