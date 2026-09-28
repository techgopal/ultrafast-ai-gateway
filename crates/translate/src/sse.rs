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

#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
    /// The last event ended on a `\r` that was the final buffered byte. If
    /// the next byte is `\n` it belongs to that same line ending.
    skip_lf: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut chunk = chunk;
        if self.skip_lf && !chunk.is_empty() {
            self.skip_lf = false;
            if chunk[0] == b'\n' {
                chunk = &chunk[1..];
            }
        }
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(boundary) = find_boundary(&self.buf) {
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
        out
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

/// Finds the first blank line. Lines end with `\n`, `\r\n` or `\r`.
fn find_boundary(buf: &[u8]) -> Option<Boundary> {
    let mut line_start = 0;
    loop {
        let pos = line_start
            + buf[line_start..]
                .iter()
                .position(|b| *b == b'\n' || *b == b'\r')?;
        let crlf = buf[pos] == b'\r' && buf.get(pos + 1) == Some(&b'\n');
        let next = pos + if crlf { 2 } else { 1 };
        if pos == line_start {
            return Some(Boundary {
                block_len: line_start,
                consumed: next,
                ends_on_trailing_cr: buf[pos] == b'\r' && next == buf.len(),
            });
        }
        line_start = next;
    }
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
}
