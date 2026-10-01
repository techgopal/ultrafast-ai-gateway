//! Server-sent events parser that works on bytes.
//!
//! Bytes are buffered until a full event (terminated by a blank line) is
//! present. Lines may end with `\n`, `\r\n` or a bare `\r`, in any mix.
//! Only complete events are decoded as text, so a chunk boundary
//! inside a multi-byte character cannot corrupt it.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// The largest event, in bytes, that a parser made with [`SseParser::new`]
/// buffers. A provider that never ends an event cannot grow the buffer
/// beyond this.
pub const MAX_EVENT_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub struct SseParser {
    buf: Vec<u8>,
    /// The last event ended on a `\r` that was the final buffered byte. If
    /// the next byte is `\n` it belongs to that same line ending.
    skip_lf: bool,
    /// Where the line being scanned starts in `buf`.
    line_start: usize,
    /// No line ending lies between `line_start` and this position, so the
    /// next scan starts here instead of at the first byte.
    scan_pos: usize,
    max_event_bytes: usize,
    /// An event went over the limit. Nothing is parsed afterwards.
    overflowed: bool,
}

impl Default for SseParser {
    fn default() -> Self {
        Self::with_max_event_bytes(MAX_EVENT_BYTES)
    }
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_event_bytes(max_event_bytes: usize) -> Self {
        Self {
            buf: Vec::new(),
            skip_lf: false,
            line_start: 0,
            scan_pos: 0,
            max_event_bytes,
            overflowed: false,
        }
    }

    /// True once an event went over the size limit. The parser has dropped
    /// its buffer and every later `feed` returns nothing.
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut out = Vec::new();
        if self.overflowed {
            return out;
        }
        let mut chunk = chunk;
        if self.skip_lf && !chunk.is_empty() {
            self.skip_lf = false;
            if chunk[0] == b'\n' {
                chunk = &chunk[1..];
            }
        }
        self.buf.extend_from_slice(chunk);
        while let Some(boundary) = self.next_boundary() {
            if boundary.block_len > self.max_event_bytes {
                self.overflow();
                return out;
            }
            self.skip_lf = boundary.ends_on_trailing_cr;
            let block: Vec<u8> = self
                .buf
                .drain(..boundary.consumed)
                .take(boundary.block_len)
                .collect();
            if let Some(ev) = parse_block(&block) {
                out.push(ev);
            }
        }
        // What is left is the start of one event.
        if self.buf.len() > self.max_event_bytes {
            self.overflow();
        }
        out
    }

    fn overflow(&mut self) {
        self.overflowed = true;
        self.skip_lf = false;
        self.line_start = 0;
        self.scan_pos = 0;
        self.buf = Vec::new();
    }

    /// Finds the first blank line. Lines end with `\n`, `\r\n` or `\r`.
    /// Remembers how far it got, so bytes are not scanned again on the next
    /// call.
    fn next_boundary(&mut self) -> Option<Boundary> {
        loop {
            let found = self.buf[self.scan_pos..]
                .iter()
                .position(|b| *b == b'\n' || *b == b'\r');
            let Some(offset) = found else {
                self.scan_pos = self.buf.len();
                return None;
            };
            let pos = self.scan_pos + offset;
            let cr = self.buf[pos] == b'\r';
            let last = pos + 1 == self.buf.len();
            let blank = pos == self.line_start;
            if cr && last && !blank {
                // A `\n` completing this line ending may still arrive.
                self.scan_pos = pos;
                return None;
            }
            let crlf = cr && self.buf.get(pos + 1) == Some(&b'\n');
            let next = pos + if crlf { 2 } else { 1 };
            if blank {
                let boundary = Boundary {
                    block_len: self.line_start,
                    consumed: next,
                    ends_on_trailing_cr: cr && last,
                };
                self.line_start = 0;
                self.scan_pos = 0;
                return Some(boundary);
            }
            self.line_start = next;
            self.scan_pos = next;
        }
    }
}

struct Boundary {
    /// Length of the event's lines, before the blank line.
    block_len: usize,
    /// Bytes to remove from the buffer: the block and the blank line.
    consumed: usize,
    /// The blank line is a `\r` at the very end of the buffer, so a `\n`
    /// completing it may still arrive.
    ends_on_trailing_cr: bool,
}

fn parse_block(block: &[u8]) -> Option<SseEvent> {
    let text = String::from_utf8_lossy(block);
    let mut event = None;
    let mut data: Vec<&str> = Vec::new();
    for line in text.split(['\n', '\r']) {
        // Splitting `\r\n` leaves an empty piece, which is skipped here.
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(v) = line.strip_prefix("data:") {
            data.push(v.strip_prefix(' ').unwrap_or(v));
        } else if let Some(v) = line.strip_prefix("event:") {
            event = Some(v.trim().to_string());
        }
    }
    if data.is_empty() && event.is_none() {
        return None;
    }
    Some(SseEvent {
        event,
        data: data.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(input: &[u8]) -> Vec<SseEvent> {
        SseParser::new().feed(input)
    }

    fn ev(event: Option<&str>, data: &str) -> SseEvent {
        SseEvent {
            event: event.map(str::to_string),
            data: data.to_string(),
        }
    }

    fn assert_split_invariant(input: &[u8], expected: &[SseEvent]) {
        assert_eq!(all(input), expected, "fed whole");
        for i in 0..=input.len() {
            let mut p = SseParser::new();
            let mut got = p.feed(&input[..i]);
            got.extend(p.feed(&input[i..]));
            assert_eq!(got, expected, "split at byte {i}");
        }
        let mut p = SseParser::new();
        let mut got = Vec::new();
        for b in input {
            got.extend(p.feed(&[*b]));
        }
        assert_eq!(got, expected, "one byte at a time");
    }

    #[test]
    fn parses_events_with_lf_and_crlf() {
        let evs = all(b"event: a\ndata: 1\n\ndata: 2\r\n\r\n");
        assert_eq!(evs.len(), 2);
        assert_eq!(
            evs[0],
            SseEvent {
                event: Some("a".into()),
                data: "1".into()
            }
        );
        assert_eq!(
            evs[1],
            SseEvent {
                event: None,
                data: "2".into()
            }
        );
    }

    #[test]
    fn joins_multiple_data_lines_and_skips_comments() {
        let evs = all(b": ping\n\ndata: a\ndata:b\n\n");
        assert_eq!(
            evs,
            vec![SseEvent {
                event: None,
                data: "a\nb".into()
            }]
        );
    }

    #[test]
    fn keeps_incomplete_event_until_more_arrives() {
        let mut p = SseParser::new();
        assert!(p.feed(b"data: par").is_empty());
        assert!(p.feed(b"tial\n").is_empty());
        assert_eq!(
            p.feed(b"\n"),
            vec![SseEvent {
                event: None,
                data: "partial".into()
            }]
        );
    }

    #[test]
    fn identical_output_for_every_split_point() {
        let input = "data: {\"t\":\"h\u{e9}llo \u{1f600}\"}\n\nevent: x\r\ndata: two\r\n\r\ndata: [DONE]\n\n".as_bytes();
        let expected = all(input);
        assert_eq!(expected.len(), 3);
        assert_eq!(expected[0].data, "{\"t\":\"h\u{e9}llo \u{1f600}\"}");
        for i in 0..=input.len() {
            let mut p = SseParser::new();
            let mut got = p.feed(&input[..i]);
            got.extend(p.feed(&input[i..]));
            assert_eq!(got, expected, "split at byte {i}");
        }
    }

    #[test]
    fn identical_output_one_byte_at_a_time() {
        let input = "data: \u{4f60}\u{597d}\n\ndata: b\n\n".as_bytes();
        let mut p = SseParser::new();
        let mut got = Vec::new();
        for b in input {
            got.extend(p.feed(&[*b]));
        }
        assert_eq!(got, all(input));
        assert_eq!(got[0].data, "\u{4f60}\u{597d}");
    }

    #[test]
    fn parses_events_separated_by_bare_cr() {
        assert_split_invariant(
            b"event: a\rdata: 1\rdata: 2\r\rdata: 3\r\r",
            &[ev(Some("a"), "1\n2"), ev(None, "3")],
        );
    }

    #[test]
    fn parses_events_with_mixed_line_endings() {
        assert_split_invariant(
            b"data: 1\n\r\ndata: 2\r\n\ndata: 3\r\r\ndata: 4\n\rdata: 5\r\n\r\n",
            &[
                ev(None, "1"),
                ev(None, "2"),
                ev(None, "3"),
                ev(None, "4"),
                ev(None, "5"),
            ],
        );
    }

    #[test]
    fn crlf_split_across_chunks_is_one_line_ending() {
        let mut p = SseParser::new();
        assert!(p.feed(b"data: 1\r").is_empty());
        assert_eq!(p.feed(b"\ndata: 2\r\n\r"), vec![ev(None, "1\n2")]);
        assert_eq!(p.feed(b"\ndata: 3\n\n"), vec![ev(None, "3")]);
    }

    #[test]
    fn oversized_partial_event_sets_the_overflow_flag_and_ends_parsing() {
        let mut p = SseParser::with_max_event_bytes(16);
        assert_eq!(p.feed(b"data: ok\n\n"), vec![ev(None, "ok")]);
        assert!(!p.overflowed());
        assert!(p.feed(b"data: 0123456789").is_empty());
        assert!(!p.overflowed());
        assert!(p.feed(b"0").is_empty());
        assert!(p.overflowed());
        // Nothing is parsed afterwards, not even a valid event.
        assert!(p.feed(b"\n\ndata: late\n\n").is_empty());
        assert!(p.overflowed());
    }

    #[test]
    fn events_before_an_oversized_one_are_still_returned() {
        let mut p = SseParser::with_max_event_bytes(16);
        let got = p.feed(b"data: 1\n\ndata: 2\n\ndata: 0123456789abcdef\n\ndata: 3\n\n");
        assert_eq!(got, vec![ev(None, "1"), ev(None, "2")]);
        assert!(p.overflowed());
    }

    #[test]
    fn overflow_does_not_depend_on_how_the_input_is_split() {
        let input = b"data: 1\n\ndata: 0123456789abcdef\n\ndata: 3\n\n";
        for i in 0..=input.len() {
            let mut p = SseParser::with_max_event_bytes(16);
            let mut got = p.feed(&input[..i]);
            got.extend(p.feed(&input[i..]));
            assert_eq!(got, vec![ev(None, "1")], "split at byte {i}");
            assert!(p.overflowed(), "split at byte {i}");
        }
    }

    #[test]
    fn event_of_exactly_the_limit_is_accepted() {
        // The first event, with its line ending, is 16 bytes.
        let input = b"data: 012345678\n\ndata: b\n\n";
        for i in 0..=input.len() {
            let mut p = SseParser::with_max_event_bytes(16);
            let mut got = p.feed(&input[..i]);
            got.extend(p.feed(&input[i..]));
            assert_eq!(got, vec![ev(None, "012345678"), ev(None, "b")], "split {i}");
            assert!(!p.overflowed(), "split at byte {i}");
        }
    }

    #[test]
    fn default_limit_is_one_mebibyte() {
        assert_eq!(MAX_EVENT_BYTES, 1024 * 1024);
        let mut p = SseParser::new();
        let mut big = b"data: ".to_vec();
        big.resize(MAX_EVENT_BYTES, b'x');
        assert!(p.feed(&big).is_empty());
        assert!(!p.overflowed());
        assert!(p.feed(b"x").is_empty());
        assert!(p.overflowed());
    }

    #[test]
    fn long_event_fed_in_small_chunks_is_parsed() {
        let mut p = SseParser::new();
        let line = format!("data: {}\r\n", "y".repeat(4000));
        let mut input = line.repeat(20).into_bytes();
        input.extend_from_slice(b"\r\n");
        let mut got = Vec::new();
        for chunk in input.chunks(7) {
            got.extend(p.feed(chunk));
        }
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].data.len(), 20 * 4000 + 19);
    }
}
