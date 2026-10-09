//! Streaming scanner with hold-back.
//!
//! Text arrives in arbitrary chunks. The scanner keeps the raw text it has not
//! released yet (plus one released character of look-behind for word
//! boundaries) and releases only a prefix that is final: nothing in the last
//! [`HOLD_BACK_CHARS`] characters can still become, or change, a match, and the
//! cut never falls inside a match (it moves back to the match start instead),
//! so a match is never released unredacted and the concatenated releases equal
//! the redaction of the whole text. A held tail is therefore at most about
//! twice the hold-back. Matches longer than the hold-back (only possible for
//! regex rules) may be missed or split.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::scan::{self, Hit};
use super::{Action, Compiled, Direction, Outcome, HOLD_BACK_CHARS};

/// What one push lets out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Release {
    /// Redacted text safe to send now (may be empty).
    pub text: String,
    /// Set once a block rule matched; nothing is released after that.
    pub blocked: Option<(i64, String)>,
}

/// What `finish` lets out: the held-back tails, scanned a final time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FinalRelease {
    pub text: String,
    /// Tails of tool-call arguments, by call index (only non-empty ones).
    pub tools: BTreeMap<u32, String>,
    pub blocked: Option<(i64, String)>,
}

#[derive(Default)]
struct Buf {
    /// Look-behind (at most one released character) followed by unreleased text.
    hay: String,
    /// Byte index in `hay` where unreleased text starts.
    base: usize,
}

/// Byte index of the `n`th character of `hay[from..]` (its end for `n` past it).
fn nth_char_byte(hay: &str, from: usize, n: usize) -> usize {
    hay[from..]
        .char_indices()
        .nth(n)
        .map_or(hay.len(), |(i, _)| from + i)
}

enum Step {
    Text(String),
    Blocked((i64, String)),
}

fn advance(set: &[Arc<Compiled>], buf: &mut Buf, outcome: &mut Outcome, last: bool) -> Step {
    let pending = buf.hay[buf.base..].chars().count();
    if pending == 0 || (!last && pending <= HOLD_BACK_CHARS) {
        return Step::Text(String::new());
    }
    let hay = buf.hay.as_str();
    let target = if last {
        hay.len()
    } else {
        nth_char_byte(hay, buf.base, pending - HOLD_BACK_CHARS)
    };
    let hits: Vec<Hit<'_>> = scan::collect_hits(set, Direction::Output, hay, buf.base);
    // Only matches starting before the target are final: a match is at most
    // the hold-back long, so it ends before the end of the text.
    if let Some(b) = scan::first_block(hits.iter().filter(|h| h.start < target)) {
        return Step::Blocked(scan::blocked_of(b));
    }
    let floor = if last || pending <= 2 * HOLD_BACK_CHARS {
        buf.base
    } else {
        nth_char_byte(hay, buf.base, pending - 2 * HOLD_BACK_CHARS)
    };
    let mut cut = target;
    loop {
        let mut moved = false;
        for h in &hits {
            if h.start < cut && h.end > cut && h.start >= floor {
                cut = h.start;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    let before_cut = || hits.iter().filter(|h| h.start < cut);
    let applied = scan::resolve(before_cut());
    let release_end = applied.iter().map(|h| h.end).fold(cut, usize::max);
    if release_end == buf.base {
        return Step::Text(String::new());
    }
    for h in before_cut().filter(|h| h.action == Some(Action::Flag)) {
        outcome.add_flag(h.guardrail.id, &h.rule.id);
    }
    for h in &applied {
        outcome.add_redaction(h.label);
    }
    let text = scan::render(hay, buf.base, release_end, &applied);
    drop(applied);
    drop(hits);
    // keep one released character so word boundaries still see it
    let keep_from = buf.hay[..release_end]
        .char_indices()
        .next_back()
        .map_or(0, |(i, _)| i);
    buf.hay.drain(..keep_from);
    buf.base = release_end - keep_from;
    Step::Text(text)
}

pub struct StreamScanner {
    set: Vec<Arc<Compiled>>,
    text: Buf,
    tools: BTreeMap<u32, Buf>,
    outcome: Outcome,
}

impl StreamScanner {
    pub fn new(set: Vec<Arc<Compiled>>) -> Self {
        StreamScanner {
            set,
            text: Buf::default(),
            tools: BTreeMap::new(),
            outcome: Outcome::default(),
        }
    }

    fn push(&mut self, tool: Option<u32>, chunk: &str) -> Release {
        if let Some(b) = &self.outcome.blocked_by {
            return Release {
                text: String::new(),
                blocked: Some(b.clone()),
            };
        }
        let buf = match tool {
            None => &mut self.text,
            Some(i) => self.tools.entry(i).or_default(),
        };
        buf.hay.push_str(chunk);
        match advance(&self.set, buf, &mut self.outcome, false) {
            Step::Text(text) => Release {
                text,
                blocked: None,
            },
            Step::Blocked(b) => self.block(b),
        }
    }

    fn block(&mut self, b: (i64, String)) -> Release {
        self.text = Buf::default();
        self.tools.clear();
        self.outcome.blocked_by = Some(b.clone());
        Release {
            text: String::new(),
            blocked: Some(b),
        }
    }

    /// Adds a text delta; returns what is now safe to send.
    pub fn push_text(&mut self, chunk: &str) -> Release {
        self.push(None, chunk)
    }

    /// Adds an argument delta of tool call `index`; calls are scanned apart.
    pub fn push_tool_args(&mut self, index: u32, chunk: &str) -> Release {
        self.push(Some(index), chunk)
    }

    /// Scans and releases every held-back tail. After a block it releases nothing.
    pub fn finish(&mut self) -> FinalRelease {
        if let Some(b) = &self.outcome.blocked_by {
            return FinalRelease {
                blocked: Some(b.clone()),
                ..FinalRelease::default()
            };
        }
        let mut out = FinalRelease::default();
        match advance(&self.set, &mut self.text, &mut self.outcome, true) {
            Step::Text(t) => out.text = t,
            Step::Blocked(b) => return self.final_block(b),
        }
        let indexes: Vec<u32> = self.tools.keys().copied().collect();
        for i in indexes {
            let buf = self.tools.get_mut(&i).expect("key listed");
            match advance(&self.set, buf, &mut self.outcome, true) {
                Step::Text(t) if !t.is_empty() => {
                    out.tools.insert(i, t);
                }
                Step::Text(_) => {}
                Step::Blocked(b) => return self.final_block(b),
            }
        }
        self.text = Buf::default();
        self.tools.clear();
        out
    }

    fn final_block(&mut self, b: (i64, String)) -> FinalRelease {
        self.block(b.clone());
        FinalRelease {
            blocked: Some(b),
            ..FinalRelease::default()
        }
    }

    pub fn outcome(&self) -> &Outcome {
        &self.outcome
    }
}
