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
        // emails and IBANs have scanners of their own (`emails`, `ibans`)
        static EMAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new("@").expect("at regex"));
        static PHONE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"\+?\(?\d[\d ().\-]{8,24}\d").expect("phone regex"));
        static CARD: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"\d(?:[ \-]?\d){12,18}").expect("card regex"));
        static SSN: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").expect("ssn regex"));
        static IPV4: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").expect("ipv4 regex"));
        static IPV6: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?:[0-9A-Fa-f]{0,4}:){2,8}[0-9A-Fa-f:.]{0,45}").expect("ipv6 regex")
        });
        static SECRET: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(concat!(
                r"(?:sk-[A-Za-z0-9_\-]{20,200}",
                r"|AKIA[0-9A-Z]{16}\b",
                r"|ghp_[A-Za-z0-9]{36,100}",
                r"|github_pat_[A-Za-z0-9_]{22,100}",
                r"|xox[abpre]-[A-Za-z0-9\-]{10,100}",
                r"|AIza[0-9A-Za-z_\-]{35}",
                r"|uf-(?:sk|at)-[A-Za-z0-9_\-]{16,200})"
            ))
            .expect("secret regex")
        });
        match self {
            PiiType::Email => &EMAIL,
            PiiType::Phone => &PHONE,
            PiiType::CreditCard => &CARD,
            PiiType::Iban => &EMAIL,
            PiiType::UsSsn => &SSN,
            PiiType::Ipv4 => &IPV4,
            PiiType::Ipv6 => &IPV6,
            PiiType::Secret => &SECRET,
        }
    }

    /// Confirms a candidate `hay[start..end]`; returns its (possibly shorter) end.
    fn refine(self, hay: &str, start: usize, end: usize) -> Verdict {
        match self {
            PiiType::Email => Verdict::Match(end),
            PiiType::Secret => secret(hay, start, end),
            PiiType::Phone => phone(hay, start, end),
            PiiType::CreditCard => card(hay, start, end).into(),
            PiiType::Iban => Verdict::Retry,
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
    /// A private-key block with no END line yet: it runs to the end of the text.
    pub open: bool,
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
    match ty {
        PiiType::Secret => pem_blocks(hay, from, out),
        PiiType::Email => return emails(hay, from, out),
        PiiType::Iban => return ibans(hay, from, out),
        _ => {}
    }
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
                    open: false,
                });
                pos = end;
            }
            Verdict::Skip => {
                out.push(Span {
                    start: m.start(),
                    end: m.end(),
                    matched: false,
                    open: false,
                });
                pos = m.end();
            }
            _ => {
                out.push(Span {
                    start: m.start(),
                    end: m.end(),
                    matched: false,
                    open: false,
                });
                pos = super::scan::next_char(hay, m.start());
            }
        }
    }
}

/// Letters, digits and combining marks of any script.
fn is_letter_or_digit(c: char) -> bool {
    c.is_alphanumeric()
        || matches!(u32::from(c), 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

fn is_mark(c: char) -> bool {
    !c.is_alphanumeric() && is_letter_or_digit(c)
}

fn is_local(c: char) -> bool {
    is_letter_or_digit(c) || matches!(c, '.' | '_' | '%' | '+' | '-')
}

fn is_label(c: char) -> bool {
    is_letter_or_digit(c) || c == '-'
}

fn is_tld(c: char) -> bool {
    (c.is_alphabetic() || is_mark(c)) && !c.is_ascii_digit()
}

/// Email addresses: 1-64 local characters, `@`, up to four labels of up to 40
/// characters, and a 2-20 letter top-level domain (at most 249 characters).
/// Local parts, labels and domains may use letters of any script.
fn emails(hay: &str, from: usize, out: &mut Vec<Span>) {
    let mut pos = from;
    while let Some(i) = hay[pos..].find('@') {
        let at = pos + i;
        pos = at + 1;
        let mut start = at;
        let mut n = 0;
        for (i, c) in hay[from..at].char_indices().rev() {
            if n == 64 || !is_local(c) {
                break;
            }
            start = from + i;
            n += 1;
        }
        // the domain region: label characters and dots, bounded
        let region: &str = {
            let rest = &hay[at + 1..];
            let mut end = 0;
            for (k, (i, c)) in rest.char_indices().enumerate() {
                if k >= 40 * 5 + 5 || !(is_label(c) || c == '.') {
                    break;
                }
                end = i + c.len_utf8();
            }
            &rest[..end]
        };
        let end = if n == 0 {
            None
        } else {
            domain_end(region).map(|len| at + 1 + len)
        };
        match end {
            Some(end) => {
                out.push(Span {
                    start,
                    end,
                    matched: true,
                    open: false,
                });
                pos = end;
            }
            None => out.push(Span {
                start,
                end: (at + 1 + region.len()).max(at + 1),
                matched: false,
                open: false,
            }),
        }
    }
}

/// Length in bytes of the longest valid domain at the start of `region`.
fn domain_end(region: &str) -> Option<usize> {
    let mut segments: Vec<(usize, usize)> = Vec::new(); // byte ranges
    let mut at = 0;
    for part in region.split('.') {
        segments.push((at, at + part.len()));
        at += part.len() + 1;
    }
    for count in (2..=segments.len().min(5)).rev() {
        let ok = segments[..count].iter().enumerate().all(|(k, &(s, e))| {
            let seg = &region[s..e];
            let chars = seg.chars().count();
            if k + 1 < count {
                (1..=40).contains(&chars)
            } else {
                (2..=20).contains(&chars) && seg.chars().all(is_tld)
            }
        });
        if ok {
            return Some(segments[count - 1].1);
        }
    }
    None
}

static PEM_BEGIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"-----BEGIN (?:[A-Z]+ )*PRIVATE KEY(?: BLOCK)?-----").expect("pem begin regex")
});

/// The END line of a private-key block.
pub(crate) static PEM_END: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"-----END (?:[A-Z]+ )*PRIVATE KEY(?: BLOCK)?-----").expect("pem end regex")
});

/// Longest END line, in bytes (generous): how much a swallowing stream keeps
/// so an END line split across chunks is still seen.
pub(crate) const PEM_END_KEEP: usize = 100;

/// Private-key blocks, whole from the BEGIN line to the END line. A block
/// without an END line runs to the end of the text and is `open`: a stream
/// then swallows everything up to the END line.
fn pem_blocks(hay: &str, from: usize, out: &mut Vec<Span>) {
    let mut pos = from;
    while let Some(b) = PEM_BEGIN.find_at(hay, pos) {
        match PEM_END.find_at(hay, b.end()) {
            Some(e) => {
                out.push(Span {
                    start: b.start(),
                    end: e.end(),
                    matched: true,
                    open: false,
                });
                pos = e.end();
            }
            None => {
                out.push(Span {
                    start: b.start(),
                    end: hay.len(),
                    matched: true,
                    open: true,
                });
                return;
            }
        }
    }
}

fn secret(hay: &str, start: usize, end: usize) -> Verdict {
    // `_` may precede (`key_sk-...`); a letter or digit means a longer word
    if is_alnum(before(hay, start)) {
        return Verdict::Retry;
    }
    Verdict::Match(end)
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
    if is_ipv4_shape(cand) || starts_with_date(cand) || !phone_grouping(cand) {
        return None;
    }
    let (prev, next) = (before(hay, start), after(hay, end));
    if is_alnum(prev) || is_alnum(next) || prev == Some(':') || next == Some(':') {
        return None;
    }
    Some(end)
}

/// Whether the digits and separators have the shape of a phone number: an
/// international `+`, a parenthesised area code, a North American 3-3-4
/// grouping, or a national number with a leading 0 in groups of two or more.
/// Decimals, coordinates, ISBNs, order numbers and plain id runs have none.
fn phone_grouping(cand: &str) -> bool {
    static PAREN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:\d{1,3}[ .\-])?\(\d{2,4}\)[ .\-]?\d").expect("paren regex")
    });
    if cand.starts_with('+') {
        return true;
    }
    if PAREN.is_match(cand) {
        return true;
    }
    let mut groups: Vec<usize> = Vec::new();
    let mut seps: Vec<char> = Vec::new();
    let mut run = 0usize;
    for c in cand.chars() {
        if c.is_ascii_digit() {
            run += 1;
        } else {
            if run > 0 {
                groups.push(run);
                run = 0;
            }
            seps.push(c);
        }
    }
    if run > 0 {
        groups.push(run);
    }
    // one separator character between groups, nothing else
    if seps.len() + 1 != groups.len() || !seps.iter().all(|c| matches!(c, ' ' | '.' | '-' | '/')) {
        return false;
    }
    let same = seps.windows(2).all(|w| w[0] == w[1]);
    let nanp = same && (groups == [3, 3, 4] || groups == [1, 3, 3, 4]);
    let national = cand.starts_with('0') && groups.len() >= 2 && groups.iter().all(|g| *g >= 2);
    nanp || national
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

/// Reference Luhn check (the detector computes the sums inline); tests compare.
#[cfg(test)]
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
    // One pass: Luhn sums for both parities are kept as digits arrive, so each
    // possible length (13..=19 digits) is checked in constant time; the
    // longest valid one wins. `plain[p]`/`doubled[p]` sum the digits at even
    // (p = 0) and odd (p = 1) positions as they are and doubled.
    let (mut plain, mut doubled) = ([0u32; 2], [0u32; 2]);
    let mut count = 0usize;
    let mut best = None;
    for (i, c) in hay.as_bytes()[start..end].iter().enumerate() {
        if !c.is_ascii_digit() {
            continue;
        }
        let d = u32::from(c - b'0');
        let p = count % 2;
        plain[p] += d;
        doubled[p] += if d > 4 { 2 * d - 9 } else { 2 * d };
        count += 1;
        if count >= 13 {
            // from the right, the last digit is not doubled: the positions
            // with the parity of the last one are plain, the others doubled
            let last = (count - 1) % 2;
            let sum = plain[last] + doubled[1 - last];
            let e = start + i + 1;
            if sum % 10 == 0 && !is_digit(after(hay, e)) {
                best = Some(e);
            }
        }
        if count == 19 {
            break;
        }
    }
    best
}

#[cfg(test)]
pub(crate) fn iban_valid(s: &str) -> bool {
    iban_valid_bytes(s.bytes())
}

#[cfg(test)]
fn iban_valid_bytes(chars: impl Iterator<Item = u8> + Clone) -> bool {
    let n = chars.clone().count();
    let mut head = chars.clone().take(4);
    let (a, b, c, d) = (head.next(), head.next(), head.next(), head.next());
    let ok_head = matches!((a, b, c, d), (Some(a), Some(b), Some(c), Some(d))
        if a.is_ascii_uppercase() && b.is_ascii_uppercase() && c.is_ascii_digit() && d.is_ascii_digit());
    if !(15..=34).contains(&n)
        || !ok_head
        || !chars
            .clone()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return false;
    }
    let mut rem = 0u32;
    for c in chars.clone().skip(4).chain(chars.take(4)) {
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

/// IBANs: two capitals, two digits and 11-30 more capitals or digits, with a
/// single space allowed before any of them. A byte scan, because a regex pays
/// its per-call cost at every capital pair in text like `AB12 CD34 ...`.
fn ibans(hay: &str, from: usize, out: &mut Vec<Span>) {
    let b = hay.as_bytes();
    let alnum = |c: u8| c.is_ascii_uppercase() || c.is_ascii_digit();
    let mut i = from;
    while i + 4 <= b.len() {
        if !(b[i].is_ascii_uppercase()
            && b[i + 1].is_ascii_uppercase()
            && b[i + 2].is_ascii_digit()
            && b[i + 3].is_ascii_digit())
        {
            i += 1;
            continue;
        }
        let (mut j, mut count) = (i + 4, 0);
        while count < 30 {
            let k = if b.get(j) == Some(&b' ') { j + 1 } else { j };
            if k < b.len() && alnum(b[k]) {
                j = k + 1;
                count += 1;
            } else {
                break;
            }
        }
        if count < 11 {
            i += 1;
            continue;
        }
        match iban(hay, i, j) {
            Some(end) => {
                out.push(Span {
                    start: i,
                    end,
                    matched: true,
                    open: false,
                });
                i = end;
            }
            None => {
                out.push(Span {
                    start: i,
                    end: j,
                    matched: false,
                    open: false,
                });
                i += 1;
            }
        }
    }
}

fn iban(hay: &str, start: usize, end: usize) -> Option<usize> {
    if is_alnum(before(hay, start)) {
        return None;
    }
    // One pass over the candidate (all ASCII): the mod-97 remainder of the part
    // after the first four characters is kept as characters arrive, so every
    // possible end is checked in constant time (the longest valid one wins).
    let bytes = hay.as_bytes();
    let value = |c: u8| -> u32 {
        if c.is_ascii_digit() {
            u32::from(c - b'0')
        } else {
            u32::from(c - b'A') + 10
        }
    };
    let digits = |c: u8| if c.is_ascii_digit() { 10 } else { 100 };
    let mut head = 0u32; // value of the first four characters, as digits
    let mut rem = 0u32; // remainder of the rest
    let mut count = 0usize;
    let mut best = None;
    for (i, &c) in bytes[start..end].iter().enumerate() {
        if c == b' ' {
            continue;
        }
        if count < 4 {
            head = head * digits(c) + value(c);
        } else {
            rem = (rem * digits(c) + value(c)) % 97;
        }
        count += 1;
        let e = start + i + 1;
        // 10^6 mod 97 shifts the remainder past the six digits of the head
        if (15..=34).contains(&count)
            && (rem * 10_u32.pow(6) % 97 + head % 97) % 97 == 1
            && !is_alnum(after(hay, e))
        {
            best = Some(e);
        }
    }
    best
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
    // The candidate is ASCII. Only ends where an address can be complete are
    // parsed: seven single colons, or exactly one `::` and at most seven
    // colons, or an embedded IPv4 part (a dot).
    let bytes = hay.as_bytes();
    let (mut colons, mut double, mut digit, mut dot) = (0usize, 0usize, false, false);
    let mut tries: Vec<usize> = Vec::new();
    for e in start + 1..=end {
        let c = bytes[e - 1];
        if c == b':' {
            colons += 1;
            if e - 1 > start && bytes[e - 2] == b':' {
                double += 1;
            }
        }
        digit |= c.is_ascii_digit();
        dot |= c == b'.';
        // `a::b` and `A::B` are identifiers, not addresses: need a digit or 3+ colons
        let plausible = digit || colons >= 3;
        let shaped = (double == 0 && colons == 7) || (double == 1 && colons <= 7) || dot;
        if e - start >= 2 && plausible && shaped {
            tries.push(e);
        }
    }
    for e in tries.into_iter().rev() {
        if hay[start..e].parse::<Ipv6Addr>().is_ok() {
            let next = after(hay, e);
            if is_alnum(next) || next == Some(':') {
                continue;
            }
            return Some(e);
        }
    }
    None
}
