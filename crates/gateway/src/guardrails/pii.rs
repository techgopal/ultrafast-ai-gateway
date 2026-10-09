//! Built-in PII and credential detectors.
//!
//! Each detector is a regular expression that proposes candidates plus a
//! validator that confirms one (Luhn, mod-97, octet ranges, ...) and may
//! shorten it. A rejected candidate is retried from the next character, so a
//! valid match hiding inside a longer rejected candidate is still found.
//! Every candidate is bounded well under the 256 character stream hold-back.

use std::net::Ipv6Addr;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    utoipa::ToSchema,
)]
pub enum PiiType {
    #[serde(rename = "EMAIL")]
    Email,
    #[serde(rename = "PHONE")]
    Phone,
    #[serde(rename = "CREDIT_CARD")]
    CreditCard,
    #[serde(rename = "IBAN")]
    Iban,
    #[serde(rename = "US_SSN")]
    UsSsn,
    #[serde(rename = "IPV4")]
    Ipv4,
    #[serde(rename = "IPV6")]
    Ipv6,
    #[serde(rename = "SECRET")]
    Secret,
}

impl PiiType {
    pub const ALL: [PiiType; 8] = [
        PiiType::Email,
        PiiType::Phone,
        PiiType::CreditCard,
        PiiType::Iban,
        PiiType::UsSsn,
        PiiType::Ipv4,
        PiiType::Ipv6,
        PiiType::Secret,
    ];

    /// The name used in outcomes and placeholders.
    pub fn name(self) -> &'static str {
        match self {
            PiiType::Email => "EMAIL",
            PiiType::Phone => "PHONE",
            PiiType::CreditCard => "CREDIT_CARD",
            PiiType::Iban => "IBAN",
            PiiType::UsSsn => "US_SSN",
            PiiType::Ipv4 => "IPV4",
            PiiType::Ipv6 => "IPV6",
            PiiType::Secret => "SECRET",
        }
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            PiiType::Email => "[REDACTED:EMAIL]",
            PiiType::Phone => "[REDACTED:PHONE]",
            PiiType::CreditCard => "[REDACTED:CREDIT_CARD]",
            PiiType::Iban => "[REDACTED:IBAN]",
            PiiType::UsSsn => "[REDACTED:US_SSN]",
            PiiType::Ipv4 => "[REDACTED:IPV4]",
            PiiType::Ipv6 => "[REDACTED:IPV6]",
            PiiType::Secret => "[REDACTED:SECRET]",
        }
    }

    fn candidates(self) -> &'static Regex {
        static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"[A-Za-z0-9._%+\-]{1,64}@(?:[A-Za-z0-9\-]{1,40}\.){1,4}[A-Za-z]{2,20}")
                .expect("email regex")
        });
        static PHONE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"\+?\(?\d[\d ().\-]{8,24}\d").expect("phone regex"));
        static CARD: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"\d(?:[ \-]?\d){12,18}").expect("card regex"));
        static IBAN: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"[A-Z]{2}\d{2}(?: ?[A-Z0-9]){11,30}").expect("iban regex")
        });
        static SSN: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").expect("ssn regex"));
        static IPV4: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").expect("ipv4 regex"));
        static IPV6: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?:[0-9A-Fa-f]{0,4}:){2,8}[0-9A-Fa-f:.]{0,45}").expect("ipv6 regex")
        });
        static SECRET: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(concat!(
                r"(?:\b(?:",
                r"sk-[A-Za-z0-9_\-]{20,200}",
                r"|AKIA[0-9A-Z]{16}",
                r"|ghp_[A-Za-z0-9]{36,100}",
                r"|github_pat_[A-Za-z0-9_]{22,100}",
                r"|xox[abp]-[A-Za-z0-9\-]{10,100}",
                r"|AIza[0-9A-Za-z_\-]{35}",
                r"|uf-(?:sk|at)-[A-Za-z0-9_\-]{16,200}",
                r")|-----BEGIN (?:[A-Z]+ )*PRIVATE KEY-----)"
            ))
            .expect("secret regex")
        });
        match self {
            PiiType::Email => &EMAIL,
            PiiType::Phone => &PHONE,
            PiiType::CreditCard => &CARD,
            PiiType::Iban => &IBAN,
            PiiType::UsSsn => &SSN,
            PiiType::Ipv4 => &IPV4,
            PiiType::Ipv6 => &IPV6,
            PiiType::Secret => &SECRET,
        }
    }

    /// Confirms a candidate `hay[start..end]`; returns its (possibly shorter) end.
    fn refine(self, hay: &str, start: usize, end: usize) -> Verdict {
        match self {
            PiiType::Email | PiiType::Secret => Verdict::Match(end),
            PiiType::Phone => phone(hay, start, end),
            PiiType::CreditCard => card(hay, start, end).into(),
            PiiType::Iban => iban(hay, start, end).into(),
            PiiType::UsSsn => ssn(&hay[start..end]).then_some(end).into(),
            PiiType::Ipv4 => ipv4(hay, start, end).into(),
            PiiType::Ipv6 => ipv6(hay, start, end).into(),
        }
    }
}

/// A candidate span of a detector.
pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
    pub matched: bool,
}

/// What a validator says about a candidate.
enum Verdict {
    /// A match ending here.
    Match(usize),
    /// Not a match; search again from the next character.
    Retry,
    /// Not a match, and nothing inside the candidate is one either.
    Skip,
}

impl From<Option<usize>> for Verdict {
    fn from(v: Option<usize>) -> Self {
        v.map_or(Verdict::Retry, Verdict::Match)
    }
}

/// All matches of `ty` in `hay` starting at or after `from`, in order and
/// without overlap. Look-behind (word boundaries, neighbours) sees all of `hay`.
///
/// Candidates a validator rejected are reported too (`matched == false`): the
/// stream scanner must not cut a stream inside one, or the next scan would
/// start in the middle of it and could accept a piece the whole text rejects.
pub(crate) fn find_all(ty: PiiType, hay: &str, from: usize, out: &mut Vec<Span>) {
    let re = ty.candidates();
    let mut pos = from;
    while pos <= hay.len() {
        let Some(m) = re.find_at(hay, pos) else { break };
        match ty.refine(hay, m.start(), m.end()) {
            Verdict::Match(end) if end > m.start() => {
                out.push(Span {
                    start: m.start(),
                    end,
                    matched: true,
                });
                pos = end;
            }
            Verdict::Skip => {
                out.push(Span {
                    start: m.start(),
                    end: m.end(),
                    matched: false,
                });
                pos = m.end();
            }
            _ => {
                out.push(Span {
                    start: m.start(),
                    end: m.end(),
                    matched: false,
                });
                pos = super::scan::next_char(hay, m.start());
            }
        }
    }
}

fn before(hay: &str, i: usize) -> Option<char> {
    hay[..i].chars().next_back()
}

fn after(hay: &str, i: usize) -> Option<char> {
    hay[i..].chars().next()
}

fn is_alnum(c: Option<char>) -> bool {
    c.is_some_and(char::is_alphanumeric)
}

fn is_digit(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_digit())
}

fn phone(hay: &str, start: usize, end: usize) -> Verdict {
    let cand = &hay[start..end];
    let digits = cand.bytes().filter(u8::is_ascii_digit).count();
    // too many digits: a card or an id, not a phone number, and not one hiding in a suffix
    if digits > 15 {
        return Verdict::Skip;
    }
    phone_shape(hay, start, end, digits).into()
}

fn phone_shape(hay: &str, start: usize, end: usize, digits: usize) -> Option<usize> {
    let cand = &hay[start..end];
    if digits < 10 {
        return None;
    }
    // bare digit runs are ids and timestamps; a phone has a plus or separators
    if !cand.starts_with('+') && !cand.contains([' ', '(', ')', '.', '-']) {
        return None;
    }
    if is_ipv4_shape(cand) || starts_with_date(cand) {
        return None;
    }
    let (prev, next) = (before(hay, start), after(hay, end));
    if is_alnum(prev) || is_alnum(next) || prev == Some(':') || next == Some(':') {
        return None;
    }
    Some(end)
}

fn is_ipv4_shape(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| (1..=3).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
}

fn starts_with_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 10
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit)
}

pub(crate) fn luhn(digits: &str) -> bool {
    let mut sum = 0u32;
    let mut double = false;
    for b in digits.bytes().rev() {
        if !b.is_ascii_digit() {
            return false;
        }
        let mut d = u32::from(b - b'0');
        if double {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
        double = !double;
    }
    !digits.is_empty() && sum.is_multiple_of(10)
}

fn card(hay: &str, start: usize, end: usize) -> Option<usize> {
    if is_digit(before(hay, start)) {
        return None;
    }
    let cand = &hay[start..end];
    let ends: Vec<usize> = cand
        .bytes()
        .enumerate()
        .filter(|(_, b)| b.is_ascii_digit())
        .map(|(i, _)| start + i + 1)
        .collect();
    for n in (13..=ends.len().min(19)).rev() {
        let e = ends[n - 1];
        let digits: String = hay[start..e].chars().filter(char::is_ascii_digit).collect();
        if luhn(&digits) && !is_digit(after(hay, e)) {
            return Some(e);
        }
    }
    None
}

pub(crate) fn iban_valid(s: &str) -> bool {
    let b = s.as_bytes();
    if !(15..=34).contains(&b.len())
        || !b[..2].iter().all(u8::is_ascii_uppercase)
        || !b[2..4].iter().all(u8::is_ascii_digit)
        || !b
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return false;
    }
    let mut rem = 0u32;
    for &c in b[4..].iter().chain(&b[..4]) {
        let v = if c.is_ascii_digit() {
            u32::from(c - b'0')
        } else {
            u32::from(c - b'A') + 10
        };
        rem = if v >= 10 {
            (rem * 100 + v) % 97
        } else {
            (rem * 10 + v) % 97
        };
    }
    rem == 1
}

fn iban(hay: &str, start: usize, end: usize) -> Option<usize> {
    if is_alnum(before(hay, start)) {
        return None;
    }
    let cand = &hay[start..end];
    for (i, _) in cand.char_indices().rev() {
        let e = start + i + 1; // all ASCII
        if e > end {
            continue;
        }
        let slice = &hay[start..e];
        if slice.ends_with(' ') {
            continue;
        }
        let compact: String = slice.chars().filter(|c| *c != ' ').collect();
        if iban_valid(&compact) && !is_alnum(after(hay, e)) {
            return Some(e);
        }
    }
    None
}

fn ssn(s: &str) -> bool {
    let area = &s[..3];
    area != "000" && area != "666" && !area.starts_with('9')
}

fn ipv4(hay: &str, start: usize, end: usize) -> Option<usize> {
    let cand = &hay[start..end];
    for part in cand.split('.') {
        if part.len() > 1 && part.starts_with('0') {
            return None;
        }
        if part.parse::<u16>().ok()? > 255 {
            return None;
        }
    }
    // part of a longer dotted number (version strings, 1.2.3.4.5)
    if after(hay, end) == Some('.') && is_digit(hay[end + 1..].chars().next()) {
        return None;
    }
    if before(hay, start) == Some('.') && is_digit(hay[..start - 1].chars().next_back()) {
        return None;
    }
    Some(end)
}

fn ipv6(hay: &str, start: usize, end: usize) -> Option<usize> {
    let cand = &hay[start..end];
    if !cand.bytes().any(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    if is_alnum(before(hay, start)) || before(hay, start) == Some(':') {
        return None;
    }
    for e in (start + 2..=end).rev() {
        let slice = &hay[start..e];
        if slice.parse::<Ipv6Addr>().is_ok() {
            let next = after(hay, e);
            if is_alnum(next) || next == Some(':') {
                continue;
            }
            return Some(e);
        }
    }
    None
}
