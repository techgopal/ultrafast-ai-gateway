//! Streaming scanner with hold-back.
//!
//! Text arrives in arbitrary chunks. The scanner keeps the raw text it has not
//! released yet (plus one released character of look-behind for word
//! boundaries) and releases only a prefix that is final: nothing in the last
//! [`HOLD_BACK_CHARS`] characters can still become, or change, a match, and the
//! cut never falls inside a match (it moves back to the match start instead),
//! so a match is never released unredacted and the concatenated releases equal
//! the redaction of the whole text. A held tail is therefore at most about
//! twice the hold-back (plus up to 128 characters while the text keeps
//! producing candidates, see `Buf::wait`). Matches longer than the hold-back (only possible for
//! regex rules) may be missed or split.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::pii;
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

/// New characters that make a rescan worthwhile on their own.
const RESCAN_AFTER_CHARS: usize = 32;
/// A scan with at most this many hits and rejected candidates is cheap.
const CHEAP_SCAN_HITS: usize = 16;
/// The longest a scan after an expensive one waits, in new characters.
const MAX_SCAN_WAIT_CHARS: usize = 128;
/// Characters of released text kept as look-behind for boundary checks
/// (an IPv4 address checks two characters before it).
const LOOK_BEHIND_CHARS: usize = 2;

#[derive(Default)]
struct Buf {
    /// Look-behind (up to two released characters) followed by unreleased text.
    hay: String,
    /// The look-behind of `hay` with its JSON escapes masked (same length).
    lead: String,
    /// Byte index in `hay` where unreleased text starts; always a token boundary.
    base: usize,
    /// Characters added since the last scan.
    since: usize,
    /// After a scan that found many candidates (hits or rejected candidates):
    /// how many new characters the next scan waits for, whatever arrives. 0
    /// after a cheap scan. Keeps the CPU per character bounded on text that
    /// looks like a detector's input (`12-34-`, `1:2:3:`).
    wait: usize,
    /// Inside a private-key block: everything is dropped until its END line.
    swallow: bool,
    /// The END line has passed: what is glued to it (no whitespace between)
    /// is dropped too, up to the next whitespace, because the whole text
    /// matches an address that starts in the END line as one unit.
    after_end: bool,
}

/// Byte index of the `n`th character of `hay[from..]` (its end for `n` past it).
fn nth_char_byte(hay: &str, from: usize, n: usize) -> usize {
    hay[from..]
        .char_indices()
        .nth(n)
        .map_or(hay.len(), |(i, _)| from + i)
}

/// Byte index where the last `LOOK_BEHIND_CHARS` characters of `hay[..end]` start.
fn look_behind_start(hay: &str, end: usize) -> usize {
    hay[..end]
        .char_indices()
        .rev()
        .nth(LOOK_BEHIND_CHARS - 1)
        .map_or(0, |(i, _)| i)
}

/// Byte index where the last `PEM_END_KEEP` bytes of `hay` start (a character
/// boundary): all a swallowing stream keeps, enough to see an END line that
/// arrives in pieces.
fn tail_window(hay: &str) -> usize {
    let mut from = hay.len().saturating_sub(pii::PEM_END_KEEP);
    while !hay.is_char_boundary(from) {
        from += 1;
    }
    from
}

enum Step {
    Text(String),
    Blocked((i64, String)),
}

fn advance(set: &[Arc<Compiled>], buf: &mut Buf, outcome: &mut Outcome, last: bool) -> Step {
    buf.since = 0;
    if buf.swallow {
        // the rest of a private-key block: nothing of it is ever released
        buf.hay.clear();
        return Step::Text(String::new());
    }
    let pending = buf.hay[buf.base..].chars().count();
    if pending == 0 || (!last && pending <= HOLD_BACK_CHARS) {
        return Step::Text(String::new());
    }
    // what the detectors read: the same bytes with JSON escapes blanked, and
    // with the characters of unspaced scripts blanked as well
    let mut scan = buf.lead.clone();
    scan.push_str(&scan::mask_escapes(&buf.hay[buf.base..]));
    let masked = scan::blank_scripts(&scan);
    let hay = buf.hay.as_str();
    let hays = scan::Hays {
        raw: hay,
        esc: &scan,
        masked: &masked,
    };
    let target = if last {
        hay.len()
    } else {
        nth_char_byte(hay, buf.base, pending - HOLD_BACK_CHARS)
    };
    let hits: Vec<Hit<'_>> = scan::collect_hits(set, Direction::Output, &hays, buf.base);
    buf.wait = if hits.len() <= CHEAP_SCAN_HITS {
        0
    } else {
        (hits.len() * 2).min(MAX_SCAN_WAIT_CHARS)
    };
    let mut escapes = Vec::new();
    scan::escape_spans(hay, buf.base, &mut escapes);
    // Only matches starting before the target are final: a match is at most
    // the hold-back long, so it ends before the end of the text.
    if let Some(b) = scan::first_block(hits.iter().filter(|h| h.start < target)) {
        return Step::Blocked(scan::blocked_of(b));
    }
    // Overlapping redact matches are one unit (their union): a cut never
    // falls inside one, however long the chain is.
    let unions = scan::resolve(hits.iter());
    // A private-key block still waiting for its END line is not released:
    // text before it goes out, the union it belongs to is replaced by one
    // placeholder and the rest of the block is swallowed.
    let open = if last {
        None
    } else {
        unions.iter().find(|a| a.open && a.start < target)
    };
    let floor = if last || pending <= 2 * HOLD_BACK_CHARS {
        buf.base
    } else {
        nth_char_byte(hay, buf.base, pending - 2 * HOLD_BACK_CHARS)
    };
    let mut cut = open.map_or(target, |m| m.start.min(target));
    let others: Vec<(usize, usize)> = hits
        .iter()
        .filter(|h| h.action != Some(Action::Redact))
        .map(|h| (h.start, h.end))
        .chain(escapes.iter().copied())
        .collect();
    for _ in 0..16 {
        let mut moved = false;
        let spans = unions
            .iter()
            .map(|a| (a.start, a.end))
            .chain(others.iter().copied());
        for (start, end) in spans {
            if start < cut && end > cut {
                // A span that began before the floor is not chased: a union
                // straddling the cut is rendered whole anyway (`release_end`
                // is the end of the last union applied), and moving the cut
                // past it could step over an open private-key block.
                if start >= floor {
                    cut = start;
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
    }
    let before_cut = || hits.iter().filter(|h| h.start < cut);
    let applied: Vec<&scan::Applied<'_>> = unions.iter().filter(|a| a.start < cut).collect();
    let release_end = applied.iter().map(|a| a.end).fold(cut, usize::max);
    let swallow_now = open.filter(|m| release_end == m.start);
    if release_end == buf.base && swallow_now.is_none() {
        return Step::Text(String::new());
    }
    for h in before_cut().filter(|h| h.action == Some(Action::Flag)) {
        outcome.add_flag(h.guardrail.id, &h.rule.id);
    }
    for h in &applied {
        outcome.add_redaction(h.label);
    }
    let mut text = scan::render(hay, buf.base, release_end, applied.iter().copied());
    if let Some(m) = swallow_now {
        text.push_str(m.placeholder);
        outcome.add_redaction(m.label);
        // keep the end of the block seen so far: its END line may be half received
        let keep = tail_window(&buf.hay);
        buf.hay.drain(..keep);
        buf.lead.clear();
        buf.base = 0;
        buf.swallow = true;
        return Step::Text(text);
    }
    drop(applied);
    drop(hits);
    // keep the last released characters so word boundaries still see them
    let keep_from = look_behind_start(&buf.hay, release_end);
    buf.lead = scan[keep_from..release_end].to_string();
    buf.hay.drain(..keep_from);
    buf.base = release_end - keep_from;
    Step::Text(text)
}

/// Adds `chunk` to `buf` and scans when that is worthwhile: after a few dozen
/// new characters or as soon as a non-letter arrives (a word may be complete).
fn push_chunk(set: &[Arc<Compiled>], buf: &mut Buf, outcome: &mut Outcome, chunk: &str) -> Step {
    buf.hay.push_str(chunk);
    if buf.swallow {
        if !buf.after_end {
            let Some(end) = pii::PEM_END.find(&buf.hay).map(|m| m.end()) else {
                let keep = tail_window(&buf.hay);
                buf.hay.drain(..keep);
                return Step::Text(String::new());
            };
            // the block ends here; keep what boundary checks look behind at
            let keep_from = look_behind_start(&buf.hay, end);
            buf.hay.drain(..keep_from);
            buf.base = end - keep_from;
            buf.after_end = true;
        }
        // what is glued to the END line goes with it, up to whitespace
        let glued = &buf.hay[buf.base..];
        let Some(space) = glued.find(char::is_whitespace) else {
            buf.hay.truncate(buf.base);
            return Step::Text(String::new());
        };
        buf.hay.drain(buf.base..buf.base + space);
        // what follows is ordinary text again
        buf.lead = buf.hay[..buf.base].to_string();
        buf.swallow = false;
        buf.after_end = false;
        buf.since = RESCAN_AFTER_CHARS;
    } else {
        buf.since += chunk.chars().count();
    }
    let due = if buf.wait == 0 {
        buf.since >= RESCAN_AFTER_CHARS || chunk.chars().any(|c| !c.is_alphanumeric())
    } else {
        buf.since >= buf.wait
    };
    if due {
        advance(set, buf, outcome, false)
    } else {
        Step::Text(String::new())
    }
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
        match push_chunk(&self.set, buf, &mut self.outcome, chunk) {
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
