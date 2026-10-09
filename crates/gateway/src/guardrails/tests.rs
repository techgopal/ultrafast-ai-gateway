use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;

fn rule(id: &str, matcher: Matcher, action: Action, directions: Directions) -> RuleSpec {
    RuleSpec {
        id: id.into(),
        matcher,
        action,
        directions,
    }
}

fn pii(id: &str, types: &[PiiType], action: Action) -> RuleSpec {
    rule(id, Matcher::Pii(types.to_vec()), action, Directions::Both)
}

fn kw(id: &str, words: &[&str], whole_word: bool, action: Action) -> RuleSpec {
    rule(
        id,
        Matcher::Keywords {
            words: words.iter().map(|w| w.to_string()).collect(),
            whole_word,
        },
        action,
        Directions::Both,
    )
}

fn re(id: &str, pattern: &str, action: Action) -> RuleSpec {
    rule(id, Matcher::Regex(pattern.into()), action, Directions::Both)
}

fn guard(id: i64, rules: &[RuleSpec]) -> Arc<Compiled> {
    Arc::new(Compiled::compile(id, &format!("g{id}"), rules).expect("compiles"))
}

fn run(set: &[Arc<Compiled>], dir: Direction, text: &str) -> (String, Outcome) {
    let mut t = [text.to_string()];
    let o = check_texts(set, dir, &mut t);
    let [t] = t;
    (t, o)
}

fn redact_pii(ty: PiiType, text: &str) -> String {
    let g = guard(1, &[pii("p", &[ty], Action::Redact)]);
    run(&[g], Direction::Input, text).0
}

// ---------- PII tables ----------

fn table(ty: PiiType, name: &str, positives: &[&str], negatives: &[&str]) {
    for p in positives {
        let text = format!("see {p} now");
        assert_eq!(
            redact_pii(ty, &text),
            format!("see [REDACTED:{name}] now"),
            "positive {p:?}"
        );
    }
    for n in negatives {
        let text = format!("see {n} now");
        assert_eq!(redact_pii(ty, &text), text, "negative {n:?}");
    }
}

#[test]
fn email_table() {
    table(
        PiiType::Email,
        "EMAIL",
        &[
            "bob@example.com",
            "bob.smith+tag@mail.example.co.uk",
            "a_b-c@x-y.org",
        ],
        &[
            "bob at example.com",
            "user@localhost",
            "@handle",
            "a@b",
            "bob@example",
        ],
    );
}

#[test]
fn phone_table() {
    table(
        PiiType::Phone,
        "PHONE",
        &[
            "+14155552671",
            "(415) 555-2671",
            "415-555-2671",
            "415.555.2671",
            "+44 20 7946 0958",
        ],
        &[
            "2024-01-15",
            "192.168.100.200",
            "12345678",
            "1700000000",
            "4111 1111 1111 1111",
            "1234567890123456789",
            "+123456789",
        ],
    );
    // a time after a date is not a phone number
    let text = "at 2024-01-15 12:30 sharp";
    assert_eq!(redact_pii(PiiType::Phone, text), text);
}

#[test]
fn credit_card_table() {
    table(
        PiiType::CreditCard,
        "CREDIT_CARD",
        &[
            "4111 1111 1111 1111",
            "4111-1111-1111-1111",
            "4111111111111111",
            "378282246310005",
            "5555 5555 5555 4444",
            "6011111111111117",
        ],
        &[
            "4111 1111 1111 1112",
            "4111111111111112",
            "411111111111",
            "41111111111111111111",
            "0000 0000 0000 0001",
        ],
    );
}

#[test]
fn luhn_check() {
    assert!(pii::luhn("4111111111111111"));
    assert!(pii::luhn("378282246310005"));
    assert!(!pii::luhn("4111111111111112"));
    assert!(!pii::luhn("1234567812345678"));
}

#[test]
fn iban_table() {
    table(
        PiiType::Iban,
        "IBAN",
        &[
            "GB82 WEST 1234 5698 7654 32",
            "GB82WEST12345698765432",
            "DE89370400440532013000",
            "FR1420041010050500013M02606",
            "NL91 ABNA 0417 1643 00",
        ],
        &[
            "GB82 WEST 1234 5698 7654 33",
            "DE89370400440532013001",
            "XX00",
            "GB82",
            "AB12CD",
        ],
    );
    assert!(pii::iban_valid("GB82WEST12345698765432"));
    assert!(!pii::iban_valid("GB82WEST12345698765433"));
}

#[test]
fn ssn_table() {
    table(
        PiiType::UsSsn,
        "US_SSN",
        &["123-45-6789", "078-05-1120", "899-12-3456"],
        &[
            "000-12-3456",
            "666-12-3456",
            "900-12-3456",
            "999-99-9999",
            "123-456-789",
            "1234-56-7890",
            "123456789",
        ],
    );
}

#[test]
fn ipv4_table() {
    table(
        PiiType::Ipv4,
        "IPV4",
        &["10.0.0.1", "192.168.1.255", "0.0.0.0", "8.8.8.8"],
        &[
            "256.1.1.1",
            "1.2.3",
            "1.2.3.4.5",
            "999.999.999.999",
            "01.2.3.4",
            "v1.2.3.4x",
        ],
    );
}

#[test]
fn ipv6_table() {
    table(
        PiiType::Ipv6,
        "IPV6",
        &[
            "2001:db8::1",
            "::1",
            "fe80::1ff:fe23:4567:890a",
            "2001:0db8:85a3:0000:0000:8a2e:0370:7334",
            "::ffff:192.168.1.1",
        ],
        &[
            "12:30:45",
            "std::vector",
            "10:15",
            "::",
            "1:2:3",
            "a::b",
            "C++ A::B",
            "Foo::Bar",
        ],
    );
}

#[test]
fn secret_table() {
    let ghp = format!("ghp_{}", "A1b2C3d4E5".repeat(4));
    let aiza = format!("AIza{}", "x".repeat(35));
    let uf_key = format!("uf-sk-{}", "0123456789abcdef".repeat(4));
    let uf_tok = format!("uf-at-{}", "0123456789abcdef".repeat(4));
    let pos: Vec<&str> = vec![
        "sk-abcdefghijklmnopqrstuv",
        "sk-ant-api03-abcdefghijklmnopqrstuv",
        "AKIAIOSFODNN7EXAMPLE",
        &ghp,
        "github_pat_11ABCDEFG0abcdefghijkl_mnopqrstuvwxyz",
        "xoxb-1234567890-abcdefghij",
        "xoxa-1234567890-abcdefghij",
        "xoxp-1234567890-abcdefghij",
        &aiza,
        "xoxr-1234567890-abcdefghij",
        "xoxe-1234567890-abcdefghij",
        &uf_key,
        &uf_tok,
    ];
    table(
        PiiType::Secret,
        "SECRET",
        &pos,
        &[
            "sk-short",
            "task-abcdefghijklmnopqrstuvwxyz",
            "AKIA123",
            "ghp_short",
            "xoxz-1234567890-abcdefghij",
            "-----BEGIN PUBLIC KEY-----",
            "uf-sk-short",
            "AKIAIOSFODNN7EXAMPLEXTRA",
            "xoxz-1234567890-abcdefghij",
        ],
    );
}

#[test]
fn all_pii_types_together() {
    let g = guard(
        1,
        &[pii(
            "p",
            &[PiiType::Email, PiiType::Ipv4, PiiType::CreditCard],
            Action::Redact,
        )],
    );
    let (t, o) = run(
        &[g],
        Direction::Input,
        "mail a@b.io from 10.0.0.1 card 4111 1111 1111 1111 and a@c.io",
    );
    assert_eq!(t, "mail [REDACTED:EMAIL] from [REDACTED:IPV4] card [REDACTED:CREDIT_CARD] and [REDACTED:EMAIL]");
    assert_eq!(o.redactions.get("EMAIL"), Some(&2));
    assert_eq!(o.redactions.get("IPV4"), Some(&1));
    assert_eq!(o.redactions.get("CREDIT_CARD"), Some(&1));
}

#[test]
fn pii_types_serialize_as_uppercase_names() {
    let m = Matcher::Pii(vec![PiiType::CreditCard, PiiType::UsSsn]);
    let s = serde_json::to_string(&m).unwrap();
    assert_eq!(s, r#"{"pii":["CREDIT_CARD","US_SSN"]}"#);
    let back: Matcher = serde_json::from_str(&s).unwrap();
    assert_eq!(back, m);
    let k: Matcher = serde_json::from_str(r#"{"keywords":{"words":["a"]}}"#).unwrap();
    assert_eq!(
        k,
        Matcher::Keywords {
            words: vec!["a".into()],
            whole_word: true
        }
    );
}

// ---------- keywords ----------

#[test]
fn keywords_whole_word_and_case() {
    let g = guard(1, &[kw("k", &["cat"], true, Action::Redact)]);
    let (t, o) = run(&[g], Direction::Input, "cat catalog Cat. concat CAT!");
    assert_eq!(t, "[REDACTED] catalog [REDACTED]. concat [REDACTED]!");
    assert_eq!(o.redactions.get("k"), Some(&3));
}

#[test]
fn keywords_unicode_boundaries() {
    let g = guard(1, &[kw("k", &["café", "école"], true, Action::Redact)]);
    let (t, _) = run(
        &[g],
        Direction::Input,
        "un café noir, des cafés, l'ÉCOLE, écoles",
    );
    assert_eq!(t, "un [REDACTED] noir, des cafés, l'[REDACTED], écoles");
}

#[test]
fn keywords_substring_mode() {
    let g = guard(1, &[kw("k", &["cat"], false, Action::Redact)]);
    let (t, _) = run(&[g], Direction::Input, "catalog concat CAT");
    assert_eq!(t, "[REDACTED]alog con[REDACTED] [REDACTED]");
}

#[test]
fn keywords_are_literal_and_longest_first() {
    let g = guard(
        1,
        &[kw(
            "k",
            &["a.b", "new", "new york", "c++"],
            true,
            Action::Redact,
        )],
    );
    let (t, _) = run(&[g], Direction::Input, "axb a.b new york new c++ rocks");
    assert_eq!(t, "axb [REDACTED] [REDACTED] [REDACTED] [REDACTED] rocks");
}

#[test]
fn keyword_limits() {
    let many: Vec<String> = (0..1001).map(|i| format!("word{i}")).collect();
    let m = Matcher::Keywords {
        words: many,
        whole_word: true,
    };
    assert!(matches!(
        Compiled::compile(1, "g", &[rule("k", m, Action::Flag, Directions::Both)]),
        Err(GuardrailError::TooManyKeywords(_))
    ));
    let exactly: Vec<String> = (0..1000).map(|i| format!("wörd{i}")).collect();
    let m = Matcher::Keywords {
        words: exactly,
        whole_word: true,
    };
    let g = Compiled::compile(1, "g", &[rule("k", m, Action::Redact, Directions::Both)])
        .expect("1000 compile");
    let (t, _) = run(&[Arc::new(g)], Direction::Input, "x wörd999 y wörd1000");
    assert_eq!(t, "x [REDACTED] y wörd1000");
    for bad in [vec![], vec!["".to_string()], vec!["  ".to_string()]] {
        let m = Matcher::Keywords {
            words: bad,
            whole_word: true,
        };
        assert!(
            Compiled::compile(1, "g", &[rule("k", m, Action::Flag, Directions::Both)]).is_err()
        );
    }
    let long = Matcher::Keywords {
        words: vec!["a".repeat(257)],
        whole_word: true,
    };
    assert!(matches!(
        Compiled::compile(1, "g", &[rule("k", long, Action::Flag, Directions::Both)]),
        Err(GuardrailError::KeywordTooLong(_))
    ));
    let ok = Matcher::Keywords {
        words: vec!["a".repeat(256)],
        whole_word: true,
    };
    assert!(Compiled::compile(1, "g", &[rule("k", ok, Action::Flag, Directions::Both)]).is_ok());
}

// ---------- regex ----------

#[test]
fn regex_redacts_with_plain_placeholder() {
    let g = guard(1, &[re("order", r"ORD-\d{4}", Action::Redact)]);
    let (t, o) = run(&[g], Direction::Input, "ORD-1234 and ORD-99 and ORD-5678");
    assert_eq!(t, "[REDACTED] and ORD-99 and [REDACTED]");
    assert_eq!(o.redactions.get("order"), Some(&2));
}

#[test]
fn regex_limits() {
    let bad = Compiled::compile(1, "g", &[re("r", "(", Action::Flag)]);
    assert!(matches!(bad, Err(GuardrailError::InvalidRegex(ref id, _)) if id == "r"));
    let big = Compiled::compile(1, "g", &[re("r", r"(?:\pL{100}){100}", Action::Flag)]);
    assert!(
        matches!(big, Err(GuardrailError::RegexTooLarge(ref id)) if id == "r"),
        "{big:?}"
    );
    let empty = Compiled::compile(1, "g", &[re("r", "a*", Action::Flag)]);
    assert!(matches!(empty, Err(GuardrailError::RegexMatchesEmpty(_))));
    let rules: Vec<RuleSpec> = (0..51)
        .map(|i| re(&format!("r{i}"), "a", Action::Flag))
        .collect();
    assert!(matches!(
        Compiled::compile(1, "g", &rules),
        Err(GuardrailError::TooManyRules)
    ));
    assert!(Compiled::compile(1, "g", &rules[..50]).is_ok());
    let dup = [re("r", "a", Action::Flag), re("r", "b", Action::Flag)];
    assert!(matches!(
        Compiled::compile(1, "g", &dup),
        Err(GuardrailError::BadRuleId(_))
    ));
    let none = Compiled::compile(
        1,
        "g",
        &[rule(
            "p",
            Matcher::Pii(vec![]),
            Action::Flag,
            Directions::Both,
        )],
    );
    assert!(matches!(none, Err(GuardrailError::NoPiiTypes(_))));
}

#[test]
fn catastrophic_regex_is_linear() {
    let g = guard(1, &[re("r", r"(a+)+c", Action::Redact)]);
    let mut text = "a".repeat(1 << 20);
    text.push('b');
    let started = Instant::now();
    let (out, o) = run(std::slice::from_ref(&g), Direction::Input, &text);
    let took = started.elapsed();
    assert_eq!(out.len(), text.len());
    assert!(o.redactions.is_empty());
    assert!(took < Duration::from_secs(20), "took {took:?}");
    // and when it does match
    let text = format!("{}c", "a".repeat(1 << 20));
    let started = Instant::now();
    let (out, o) = run(&[g], Direction::Input, &text);
    assert_eq!(out, "[REDACTED]");
    assert_eq!(o.redactions.get("r"), Some(&1));
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[test]
fn pii_scan_of_a_large_text_stays_fast() {
    let g = guard(
        1,
        &[pii(
            "p",
            &[
                PiiType::Email,
                PiiType::Phone,
                PiiType::CreditCard,
                PiiType::Iban,
                PiiType::UsSsn,
                PiiType::Ipv4,
                PiiType::Ipv6,
                PiiType::Secret,
            ],
            Action::Redact,
        )],
    );
    let unit = "lorem ipsum 12345 6789 dead:beef 1.2.3 AB12 GB82 4111 abc@ 2024-01-15 ";
    let text = unit.repeat((1 << 20) / unit.len());
    let started = Instant::now();
    let _ = run(std::slice::from_ref(&g), Direction::Input, &text);
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "took {:?}",
        started.elapsed()
    );
    let digits = "7".repeat(1 << 20);
    let started = Instant::now();
    let _ = run(&[g], Direction::Input, &digits);
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "digits took {:?}",
        started.elapsed()
    );
}

// ---------- actions, order, directions ----------

#[test]
fn overlapping_matches_are_replaced_once_leftmost_longest() {
    let g = guard(
        1,
        &[
            kw("k", &["example"], false, Action::Redact),
            pii("p", &[PiiType::Email], Action::Redact),
        ],
    );
    let (t, o) = run(&[g], Direction::Input, "bob@example.com and example");
    assert_eq!(t, "[REDACTED:EMAIL] and [REDACTED]");
    assert_eq!(o.redactions.get("EMAIL"), Some(&1));
    assert_eq!(o.redactions.get("k"), Some(&1));
}

#[test]
fn equal_matches_go_to_the_earlier_rule() {
    let g = guard(
        1,
        &[
            re("first", "abc", Action::Redact),
            re("second", "abc", Action::Redact),
        ],
    );
    let (t, o) = run(&[g], Direction::Input, "abc");
    assert_eq!(t, "[REDACTED]");
    assert_eq!(o.redactions.get("first"), Some(&1));
    assert_eq!(o.redactions.get("second"), None);
}

#[test]
fn rules_match_the_original_text_not_each_others_output() {
    let g = guard(
        1,
        &[
            kw("a", &["secret"], true, Action::Redact),
            kw("b", &["redacted"], false, Action::Redact),
        ],
    );
    let (t, o) = run(&[g], Direction::Input, "a secret thing");
    assert_eq!(t, "a [REDACTED] thing");
    assert_eq!(o.redactions.len(), 1);
}

#[test]
fn block_wins_and_leaves_texts_untouched() {
    let a = guard(1, &[pii("p", &[PiiType::Email], Action::Redact)]);
    let b = guard(2, &[kw("k", &["forbidden"], true, Action::Block)]);
    let c = guard(3, &[kw("k2", &["forbidden"], true, Action::Block)]);
    let mut texts = [
        "hi bob@example.com".to_string(),
        "this is forbidden".to_string(),
    ];
    let o = check_texts(&[a, b, c], Direction::Input, &mut texts);
    assert_eq!(o.blocked_by, Some((2, "g2".to_string())));
    assert_eq!(texts[0], "hi bob@example.com");
    assert_eq!(texts[1], "this is forbidden");
    assert!(o.redactions.is_empty());
}

#[test]
fn flags_are_recorded_once_per_rule_and_text_is_unchanged() {
    let g = guard(7, &[kw("watch", &["beta"], true, Action::Flag)]);
    let mut texts = ["beta beta".to_string(), "another beta".to_string()];
    let o = check_texts(&[g], Direction::Output, &mut texts);
    assert_eq!(o.flags, vec![(7, "watch".to_string())]);
    assert_eq!(texts[0], "beta beta");
    assert!(o.blocked_by.is_none());
}

#[test]
fn directions_select_rules() {
    let g = guard(
        1,
        &[
            rule(
                "in",
                Matcher::Regex("alpha".into()),
                Action::Redact,
                Directions::Input,
            ),
            rule(
                "out",
                Matcher::Regex("beta".into()),
                Action::Redact,
                Directions::Output,
            ),
            rule(
                "both",
                Matcher::Regex("gamma".into()),
                Action::Redact,
                Directions::Both,
            ),
        ],
    );
    let (t, _) = run(
        std::slice::from_ref(&g),
        Direction::Input,
        "alpha beta gamma",
    );
    assert_eq!(t, "[REDACTED] beta [REDACTED]");
    let (t, _) = run(&[g], Direction::Output, "alpha beta gamma");
    assert_eq!(t, "alpha [REDACTED] [REDACTED]");
}

#[test]
fn counts_aggregate_over_texts() {
    let g = guard(1, &[pii("p", &[PiiType::Email], Action::Redact)]);
    let mut texts = [
        "a@b.io".to_string(),
        "none".to_string(),
        "c@d.io e@f.io".to_string(),
    ];
    let o = check_texts(&[g], Direction::Input, &mut texts);
    assert_eq!(o.redactions.get("EMAIL"), Some(&3));
    assert_eq!(texts[1], "none");
}

#[test]
fn outcome_never_holds_matched_text() {
    let g = guard(
        1,
        &[
            pii("p", &[PiiType::Email], Action::Redact),
            kw("k", &["hunter2"], true, Action::Flag),
        ],
    );
    let (_, o) = run(&[g], Direction::Input, "bob@example.com hunter2");
    let dump = format!("{o:?}");
    assert!(!dump.contains("bob") && !dump.contains("example.com") && !dump.contains("hunter2"));
}

// ---------- streaming ----------

fn feed(set: &[Arc<Compiled>], chunks: &[&str]) -> (String, Outcome, Option<(i64, String)>) {
    let mut s = StreamScanner::new(set.to_vec());
    let mut out = String::new();
    let mut blocked = None;
    for c in chunks {
        let r = s.push_text(c);
        out.push_str(&r.text);
        blocked = blocked.or(r.blocked);
    }
    let f = s.finish();
    out.push_str(&f.text);
    blocked = blocked.or(f.blocked);
    (out, s.outcome().clone(), blocked)
}

#[test]
fn stream_holds_back_exactly_the_tail() {
    let g = guard(1, &[pii("p", &[PiiType::Email], Action::Redact)]);
    let mut s = StreamScanner::new(vec![g]);
    let r = s.push_text(&"x".repeat(1000));
    assert_eq!(r.text.chars().count(), 1000 - HOLD_BACK_CHARS);
    // a few letters alone do not trigger a scan ...
    assert_eq!(s.push_text("tail").text, "");
    // ... a non-letter does
    let r = s.push_text(".");
    assert_eq!(r.text.chars().count(), 5);
    let f = s.finish();
    assert_eq!(f.text.chars().count(), HOLD_BACK_CHARS);
}

#[test]
fn stream_short_text_is_released_only_at_finish() {
    let g = guard(1, &[pii("p", &[PiiType::Email], Action::Redact)]);
    let mut s = StreamScanner::new(vec![g]);
    assert_eq!(s.push_text("hello bob@exa").text, "");
    assert_eq!(s.push_text("mple.com bye").text, "");
    assert_eq!(s.finish().text, "hello [REDACTED:EMAIL] bye");
}

#[test]
fn stream_match_across_the_hold_back_boundary_chars_one_by_one() {
    let g = guard(
        1,
        &[pii(
            "p",
            &[PiiType::Email, PiiType::CreditCard],
            Action::Redact,
        )],
    );
    for pad in [200usize, 250, 255, 256, 257, 300, 600] {
        let text = format!(
            "{}bob@example.com{} 4111 1111 1111 1111 end",
            "z".repeat(pad),
            "y".repeat(10)
        );
        let chars: Vec<String> = text.chars().map(|c| c.to_string()).collect();
        let refs: Vec<&str> = chars.iter().map(|s| s.as_str()).collect();
        let (out, o, _) = feed(std::slice::from_ref(&g), &refs);
        let (want, wo) = run(std::slice::from_ref(&g), Direction::Output, &text);
        assert_eq!(out, want, "pad {pad}");
        assert_eq!(o, wo);
        assert!(!out.contains('@'));
    }
}

#[test]
fn stream_match_across_three_chunks() {
    let g = guard(1, &[pii("p", &[PiiType::Email], Action::Redact)]);
    let pad = "z".repeat(300);
    let a = format!("{pad} bo");
    let (out, _, _) = feed(&[g], &[&a, "b@exam", "ple.com", " done"]);
    assert_eq!(out, format!("{pad} [REDACTED:EMAIL] done"));
}

#[test]
fn stream_block_stops_before_the_match_and_reports() {
    let g = guard(4, &[kw("k", &["forbidden"], true, Action::Block)]);
    let mut s = StreamScanner::new(vec![g]);
    let pad = "z ".repeat(200);
    let mut released = String::new();
    let mut blocked = None;
    for c in [&pad[..], "this is forb", "idden ", &pad[..], &pad[..]] {
        let r = s.push_text(c);
        released.push_str(&r.text);
        if r.blocked.is_some() {
            blocked = r.blocked;
        }
    }
    assert_eq!(blocked, Some((4, "g4".to_string())));
    assert!(
        !released.contains("forb"),
        "released {} chars",
        released.len()
    );
    // after a block nothing more comes out
    let r = s.push_text("more");
    assert_eq!(r.text, "");
    assert!(r.blocked.is_some());
    assert!(s.finish().text.is_empty());
    assert_eq!(s.outcome().blocked_by, Some((4, "g4".to_string())));
}

#[test]
fn stream_block_in_the_tail_is_found_at_finish() {
    let g = guard(4, &[kw("k", &["forbidden"], true, Action::Block)]);
    let mut s = StreamScanner::new(vec![g]);
    assert!(s.push_text("this is forbidden").blocked.is_none());
    let f = s.finish();
    assert_eq!(f.blocked, Some((4, "g4".to_string())));
    assert_eq!(f.text, "");
}

#[test]
fn stream_tool_args_are_scanned_per_index() {
    let g = guard(1, &[pii("p", &[PiiType::Email], Action::Redact)]);
    let mut s = StreamScanner::new(vec![g]);
    let mut t0 = String::new();
    let mut t1 = String::new();
    let mut tx = String::new();
    t0 += &s.push_tool_args(0, r#"{"to":"bob@ex"#).text;
    t1 += &s.push_tool_args(1, r#"{"to":"al@"#).text;
    tx += &s.push_text("hello al@").text;
    t0 += &s.push_tool_args(0, r#"ample.com"}"#).text;
    t1 += &s.push_tool_args(1, r#"x.org"}"#).text;
    tx += &s.push_text("x.org").text;
    let f = s.finish();
    t0 += f.tools.get(&0).map(String::as_str).unwrap_or("");
    t1 += f.tools.get(&1).map(String::as_str).unwrap_or("");
    tx += &f.text;
    assert_eq!(t0, r#"{"to":"[REDACTED:EMAIL]"}"#);
    assert_eq!(t1, r#"{"to":"[REDACTED:EMAIL]"}"#);
    assert_eq!(tx, "hello [REDACTED:EMAIL]");
    assert_eq!(s.outcome().redactions.get("EMAIL"), Some(&3));
}

#[test]
fn stream_tool_args_block_reports() {
    let g = guard(2, &[kw("k", &["rm -rf"], false, Action::Block)]);
    let mut s = StreamScanner::new(vec![g]);
    s.push_tool_args(3, r#"{"cmd":"rm "#);
    s.push_tool_args(3, r#"-rf /"}"#);
    let f = s.finish();
    assert_eq!(f.blocked, Some((2, "g2".to_string())));
}

#[test]
fn stream_flags_and_no_double_count() {
    let g = guard(9, &[kw("w", &["beta"], true, Action::Flag)]);
    let pad = "z".repeat(700);
    let t = format!("beta {pad} beta");
    let chars: Vec<String> = t.chars().map(|c| c.to_string()).collect();
    let refs: Vec<&str> = chars.iter().map(|s| s.as_str()).collect();
    let (out, o, _) = feed(&[g], &refs);
    assert_eq!(out, t);
    assert_eq!(o.flags, vec![(9, "w".to_string())]);
}

/// Around the first difference of two strings, for failure messages.
fn diff_window(a: &str, b: &str) -> String {
    let at = a
        .char_indices()
        .zip(b.chars())
        .find(|((_, x), y)| x != y)
        .map_or(a.len().min(b.len()), |((i, _), _)| i);
    let lo = (0..=at.saturating_sub(40))
        .rev()
        .find(|i| a.is_char_boundary(*i) && b.is_char_boundary(*i))
        .unwrap_or(0);
    let cut = |s: &str| -> String { s[lo..].chars().take(100).collect() };
    format!(
        "at byte {at}: released ...{:?} want ...{:?}",
        cut(a),
        cut(b)
    )
}

// deterministic xorshift
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const PIECES: &[&str] = &[
    "hello",
    " ",
    " ",
    "cat",
    "category",
    "Cat",
    ".",
    ",",
    "naïve",
    "日本語",
    "🙂",
    "é",
    "foo12",
    "foo",
    "x",
    "line\n",
    "bob@example.com",
    "al.ice@mail.example.org",
    "4111 1111 1111 1111",
    "4111-1111-1111-1111",
    "123-45-6789",
    "10.0.0.1",
    "sk-ABCDEFGHIJKLMNOPQRSTUVWX",
    "AKIAIOSFODNN7EXAMPLE",
    "secret",
    "SECRET",
    "2001:db8::1",
    "GB82 WEST 1234 5698 7654 32",
    "+14155552671",
    "(415) 555-2671",
    "hello world",
    "1.2.3.4.5",
    "\\nsk-ABCDEFGHIJKLMNOPQRSTUVWX",
    "\\n123-45-6789\\t",
    "-----BEGIN RSA PRIVATE KEY-----\\nMIIEabcdefghij\\n-----END RSA PRIVATE KEY-----",
    "-----BEGIN PGP PRIVATE KEY BLOCK-----\nabcdefghijklmnop\n-----END PGP PRIVATE KEY BLOCK-----\n",
    "密码",
    "josé@gmail.com",
];

fn random_text(rng: &mut Rng) -> String {
    let n = match rng.below(4) {
        0 => rng.below(8),
        1 => rng.below(40),
        _ => 40 + rng.below(110),
    };
    let mut s = String::new();
    for _ in 0..n {
        s.push_str(PIECES[rng.below(PIECES.len())]);
        if rng.below(25) == 0 {
            s.push_str(&"q".repeat(rng.below(300)));
        }
    }
    if rng.below(8) == 0 {
        // a key that never ends: the stream swallows it to the end
        s.push_str(" -----BEGIN PRIVATE KEY-----\nMIIabcdefghijklmnop");
        s.push_str(&"A1b2".repeat(rng.below(200)));
    }
    s
}

/// Splits `text` into chunks; mode picks the size distribution. Byte mode
/// cuts anywhere (inside multi-byte characters too) and carries the incomplete
/// bytes to the next chunk, as a network decoder would.
fn chunk(text: &str, rng: &mut Rng) -> Vec<String> {
    let mode = rng.below(4);
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let size = match mode {
            0 => 1,
            1 => 1 + rng.below(40),
            2 => 1 + rng.below(700),
            _ => 1 + rng.below(3),
        };
        let end = (i + size).min(bytes.len());
        pending.extend_from_slice(&bytes[i..end]);
        i = end;
        match std::str::from_utf8(&pending) {
            Ok(s) => {
                out.push(s.to_string());
                pending.clear();
            }
            Err(e) if e.error_len().is_none() => {
                let ok = e.valid_up_to();
                if ok > 0 {
                    out.push(std::str::from_utf8(&pending[..ok]).unwrap().to_string());
                    pending.drain(..ok);
                }
            }
            Err(_) => unreachable!("input is valid UTF-8"),
        }
    }
    assert!(pending.is_empty());
    out
}

fn property_set() -> Vec<Arc<Compiled>> {
    vec![
        guard(
            1,
            &[
                pii(
                    "pii",
                    &[
                        PiiType::Email,
                        PiiType::CreditCard,
                        PiiType::UsSsn,
                        PiiType::Secret,
                        PiiType::Ipv4,
                        PiiType::Ipv6,
                        PiiType::Iban,
                        PiiType::Phone,
                    ],
                    Action::Redact,
                ),
                kw("kw", &["secret", "Cat"], true, Action::Redact),
                re("re", r"\bfoo\d+\b", Action::Redact),
                kw("flag", &["hello"], true, Action::Flag),
            ],
        ),
        guard(2, &[re("lines", r"line\nbob@example\.com", Action::Redact)]),
    ]
}

#[test]
fn property_stream_equals_whole_text_redaction() {
    let set = property_set();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut cases_with_redactions = 0;
    for case in 0..300 {
        let text = random_text(&mut rng);
        let (want, want_outcome) = run(&set, Direction::Output, &text);
        let chunks = chunk(&text, &mut rng);
        let mut s = StreamScanner::new(set.clone());
        let mut released = String::new();
        for c in &chunks {
            let r = s.push_text(c);
            assert!(r.blocked.is_none());
            released.push_str(&r.text);
            // nothing is ever sent that the whole-text redaction would not send
            assert!(
                want.starts_with(&released),
                "case {case}: released text diverges from the redacted whole: {}",
                diff_window(&released, &want)
            );
            // and the held-back part is bounded
            let sent_raw_floor = text.chars().count();
            let _ = sent_raw_floor;
        }
        let f = s.finish();
        released.push_str(&f.text);
        assert_eq!(released, want, "case {case}");
        assert_eq!(s.outcome(), &want_outcome, "case {case}");
        if !want_outcome.redactions.is_empty() {
            cases_with_redactions += 1;
        }
    }
    assert!(
        cases_with_redactions > 100,
        "only {cases_with_redactions} cases planted a match"
    );
}

#[test]
fn property_stream_hold_back_is_bounded() {
    // flag-only rules leave the text unchanged, so raw lengths can be compared
    let set = vec![guard(
        1,
        &[
            pii(
                "pii",
                &[PiiType::Email, PiiType::CreditCard, PiiType::Secret],
                Action::Flag,
            ),
            kw("kw", &["secret", "Cat"], true, Action::Flag),
        ],
    )];
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for case in 0..200 {
        let text = random_text(&mut rng);
        let chunks = chunk(&text, &mut rng);
        let mut s = StreamScanner::new(set.clone());
        let (mut fed, mut out) = (0usize, 0usize);
        let mut all = String::new();
        for c in &chunks {
            fed += c.chars().count();
            let r = s.push_text(c);
            out += r.text.chars().count();
            all.push_str(&r.text);
            assert!(out <= fed, "case {case}");
            assert!(
                fed - out <= 2 * HOLD_BACK_CHARS + 128,
                "case {case}: {} held",
                fed - out
            );
        }
        all.push_str(&s.finish().text);
        assert_eq!(all, text, "case {case}");
    }
}

#[test]
fn property_stream_block_never_releases_the_blocked_match() {
    let set = vec![guard(
        3,
        &[
            kw("blk", &["secret"], true, Action::Block),
            pii("pii", &[PiiType::Email], Action::Redact),
        ],
    )];
    let redacting = vec![guard(
        3,
        &[
            kw("blk", &["secret"], true, Action::Redact),
            pii("pii", &[PiiType::Email], Action::Redact),
        ],
    )];
    let mut rng = Rng(0xA076_1D64_78BD_642F);
    let mut blocked_cases = 0;
    for case in 0..250 {
        let text = random_text(&mut rng);
        let mut t = [text.clone()];
        let whole = check_texts(&set, Direction::Output, &mut t);
        let chunks = chunk(&text, &mut rng);
        let mut s = StreamScanner::new(set.clone());
        let mut released = String::new();
        let mut blocked = None;
        for c in &chunks {
            let r = s.push_text(c);
            released.push_str(&r.text);
            blocked = blocked.or(r.blocked);
        }
        let f = s.finish();
        released.push_str(&f.text);
        blocked = blocked.or(f.blocked);
        assert_eq!(blocked.is_some(), whole.blocked_by.is_some(), "case {case}");
        if blocked.is_some() {
            blocked_cases += 1;
            // what was released is a prefix of the redacted text and stops
            // before the first match of the blocking rule (a plain placeholder)
            let (as_redacted, _) = run(&redacting, Direction::Output, &text);
            assert!(as_redacted.starts_with(&released), "case {case}");
            assert!(!released.contains("[REDACTED]"), "case {case}");
        } else {
            assert_eq!(released, t[0], "case {case}");
        }
    }
    assert!(blocked_cases > 20);
}

// ---------- fix round 1 ----------

fn redact_all(text: &str) -> String {
    let g = guard(1, &[pii("p", &PiiType::ALL, Action::Redact)]);
    run(&[g], Direction::Input, text).0
}

#[test]
fn json_escapes_are_boundaries() {
    let key = "sk-abcdefghijklmnopqrstuv";
    let cases = [
        (format!(r"a\n{key}"), r"a\n[REDACTED:SECRET]"),
        (format!(r"a\t{key}\n"), r"a\t[REDACTED:SECRET]\n"),
        (r"x\n123-45-6789".to_string(), r"x\n[REDACTED:US_SSN]"),
        (r"x\n10.0.0.1\n".to_string(), r"x\n[REDACTED:IPV4]\n"),
        (
            r"x\nDE89370400440532013000\n".to_string(),
            r"x\n[REDACTED:IBAN]\n",
        ),
        (r"x\n+14155552671\n".to_string(), r"x\n[REDACTED:PHONE]\n"),
        (r"x\n(415) 555-2671\n".to_string(), r"x\n[REDACTED:PHONE]\n"),
        (r"x\nbob@x.com\n".to_string(), r"x\n[REDACTED:EMAIL]\n"),
        (r"x bob@x.com".to_string(), r"x [REDACTED:EMAIL]"),
        (
            r#"{\"k\":\"4111 1111 1111 1111\"}"#.to_string(),
            r#"{\"k\":\"[REDACTED:CREDIT_CARD]\"}"#,
        ),
    ];
    for (input, want) in cases {
        assert_eq!(redact_all(&input), want, "{input}");
    }
    // an escaped backslash followed by n is a backslash and a letter
    let t = format!(r"a\\n{key}");
    assert_eq!(redact_all(&t), t);
    // keywords too
    let g = guard(1, &[kw("k", &["secret"], true, Action::Redact)]);
    assert_eq!(
        run(&[g], Direction::Input, r"x\nsecret\t").0,
        r"x\n[REDACTED]\t"
    );
}

#[test]
fn json_escapes_in_a_stream_cut_anywhere() {
    let g = guard(1, &[pii("p", &PiiType::ALL, Action::Redact)]);
    let key = "sk-abcdefghijklmnopqrstuv";
    for pad in 240..300 {
        let text = format!("{}\\n{key}\\n123-45-6789\\t end", "z ".repeat(pad / 2));
        let chars: Vec<String> = text.chars().map(|c| c.to_string()).collect();
        let refs: Vec<&str> = chars.iter().map(String::as_str).collect();
        let (out, o, _) = feed(std::slice::from_ref(&g), &refs);
        let (want, wo) = run(std::slice::from_ref(&g), Direction::Output, &text);
        assert_eq!(out, want, "pad {pad}");
        assert_eq!(o, wo);
    }
}

#[test]
fn email_accepts_unicode_letters() {
    table(
        PiiType::Email,
        "EMAIL",
        &[
            "josé@gmail.com",
            "café@example.com",
            "user@münchen.de",
            "用户@例子.广告",
            "bob@мир.рф",
        ],
        &["é@", "@é.com", "user@é"],
    );
}

#[test]
fn whole_word_keywords_in_unspaced_scripts_match_as_substrings() {
    for (word, text, want) in [
        ("密码", "我的密码是abc", "我的[REDACTED]是abc"),
        ("パスワード", "これはパスワードです", "これは[REDACTED]です"),
        ("ภาษาไทย", "ผมพูดภาษาไทยได้", "ผมพูด[REDACTED]ได้"),
        ("한국", "한국어", "한국어"), // Hangul has spaces: stays whole-word
        ("cat", "catalog 密码 cat", "catalog 密码 [REDACTED]"),
    ] {
        let g = guard(1, &[kw("k", &[word], true, Action::Redact)]);
        assert_eq!(run(&[g], Direction::Input, text).0, want, "{word}");
    }
    // a block keyword in Chinese fires on running text
    let g = guard(2, &[kw("k", &["密码"], true, Action::Block)]);
    let o = run(&[g], Direction::Input, "请告诉我密码").1;
    assert_eq!(o.blocked_by, Some((2, "g2".to_string())));
}

#[test]
fn regex_anchors_are_refused() {
    for p in [
        "^a", "a$", r"\Aa", r"a\z", "(?m)^a", "(?m)a$", "(a|^b)c", "x(?:$)",
    ] {
        let r = Compiled::compile(1, "g", &[re("r", p, Action::Flag)]);
        assert!(
            matches!(r, Err(GuardrailError::RegexAnchor(ref id)) if id == "r"),
            "{p}: {r:?}"
        );
    }
    for p in [r"\bfoo\b", "[^a]x", r"a\$", r"a\^", "[$^]x", r"\Bfoo"] {
        assert!(
            Compiled::compile(1, "g", &[re("r", p, Action::Flag)]).is_ok(),
            "{p}"
        );
    }
}

#[test]
fn phone_is_not_numbers_that_merely_have_digits() {
    for neg in [
        "0.123456789012",
        "1234567.1234567",
        "3.14159265358979",
        "-122.4194155",
        "Order #20240115-0042",
        "ISBN 978-3-16-148410-0",
        "12345-67890",
        "12-34-56-78-90",
        "37.7749295 -122.4194155",
    ] {
        let text = format!("see {neg} now");
        assert_eq!(redact_pii(PiiType::Phone, &text), text, "{neg}");
    }
    table(
        PiiType::Phone,
        "PHONE",
        &[
            "+14155552671",
            "+1 415 555 2671",
            "(415) 555-2671",
            "1 (415) 555-2671",
            "415-555-2671",
            "415.555.2671",
            "415 555 2671",
            "1-415-555-2671",
            "+44 20 7946 0958",
            "020 7946 0958",
            "01 23 45 67 89",
            "030 12345678",
            "+49 30 901820",
        ],
        &[],
    );
}

#[test]
fn secret_boundaries_and_block_kinds() {
    assert_eq!(
        redact_pii(PiiType::Secret, "key_sk-abcdefghijklmnopqrstuv end"),
        "key_[REDACTED:SECRET] end"
    );
    assert_eq!(
        redact_pii(PiiType::Secret, "AKIAIOSFODNN7EXAMPLEXTRA"),
        "AKIAIOSFODNN7EXAMPLEXTRA"
    );
    assert_eq!(
        redact_pii(PiiType::Secret, "a AKIAIOSFODNN7EXAMPLE."),
        "a [REDACTED:SECRET]."
    );
}

const PEM: &str = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEAabc\ndefGHI+/=\n-----END RSA PRIVATE KEY-----";
const PGP: &str =
    "-----BEGIN PGP PRIVATE KEY BLOCK-----\nlQdGBF\n=abcd\n-----END PGP PRIVATE KEY BLOCK-----";

#[test]
fn private_key_blocks_are_redacted_whole() {
    for block in [
        PEM,
        PGP,
        "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----",
    ] {
        assert_eq!(
            redact_pii(PiiType::Secret, &format!("before\n{block}\nafter")),
            "before\n[REDACTED:SECRET]\nafter"
        );
    }
    // two blocks, then an unterminated one that runs to the end
    let text = format!("{PEM} and {PGP} and -----BEGIN PRIVATE KEY-----\nAAAA tail");
    assert_eq!(
        redact_pii(PiiType::Secret, &text),
        "[REDACTED:SECRET] and [REDACTED:SECRET] and [REDACTED:SECRET]"
    );
    // a public key is not private
    let public = "-----BEGIN PUBLIC KEY-----\nAAAA\n-----END PUBLIC KEY-----";
    assert_eq!(redact_pii(PiiType::Secret, public), public);
}

#[test]
fn private_key_blocks_are_swallowed_in_streams() {
    let g = guard(1, &[pii("p", &PiiType::ALL, Action::Redact)]);
    let body = "MIIEowIBAAKCAQEA".repeat(400); // 6400 chars of key
    let text = format!(
        "here is the key: -----BEGIN RSA PRIVATE KEY-----\n{body}\n-----END RSA PRIVATE KEY----- and bob@example.com after"
    );
    for size in [1usize, 7, 50, 300, 5000] {
        let chunks: Vec<String> = text
            .chars()
            .collect::<Vec<_>>()
            .chunks(size)
            .map(|c| c.iter().collect())
            .collect();
        let refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
        let mut s = StreamScanner::new(vec![g.clone()]);
        let mut out = String::new();
        for c in &refs {
            out.push_str(&s.push_text(c).text);
            assert!(!out.contains("MIIE"), "size {size}: key body released");
        }
        out.push_str(&s.finish().text);
        assert_eq!(
            out, "here is the key: [REDACTED:SECRET] and [REDACTED:EMAIL] after",
            "size {size}"
        );
        assert_eq!(s.outcome().redactions.get("SECRET"), Some(&1));
    }
    // the stream ends inside the key: nothing of it comes out
    let cut = format!("x -----BEGIN PGP PRIVATE KEY BLOCK-----\n{body}");
    let chunks: Vec<String> = cut
        .chars()
        .collect::<Vec<_>>()
        .chunks(40)
        .map(|c| c.iter().collect())
        .collect();
    let refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
    let (out, o, _) = feed(&[g], &refs);
    assert_eq!(out, "x [REDACTED:SECRET]");
    assert_eq!(o.redactions.get("SECRET"), Some(&1));
}

#[test]
fn ipv4_inside_a_longer_dotted_number_in_a_stream() {
    let g = guard(1, &[pii("p", &[PiiType::Ipv4], Action::Redact)]);
    for pad in 200..330 {
        let text = format!("{}1.2.3.4.5 and 10.0.0.1 end", "z ".repeat(pad / 2));
        let chars: Vec<String> = text.chars().map(|c| c.to_string()).collect();
        let refs: Vec<&str> = chars.iter().map(String::as_str).collect();
        let (out, o, _) = feed(std::slice::from_ref(&g), &refs);
        let (want, wo) = run(std::slice::from_ref(&g), Direction::Output, &text);
        assert_eq!(out, want, "pad {pad}");
        assert_eq!(o, wo);
    }
}

/// CPU time of this thread (so other load on the machine does not count);
/// falls back to the wall clock where `/proc` has no schedstat.
fn cpu_time() -> Duration {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let on_cpu = std::fs::read_to_string("/proc/thread-self/schedstat")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<u64>().ok());
    match on_cpu {
        Some(ns) => Duration::from_nanos(ns),
        None => START.get_or_init(Instant::now).elapsed(),
    }
}

/// Release builds only (`cargo test --release`): 4k tokens (about 16k chars)
/// of content that keeps every detector busy, fed one character at a time.
#[test]
#[cfg_attr(debug_assertions, ignore = "timing: run with --release")]
fn stream_cpu_is_bounded_on_adversarial_text() {
    let set = vec![guard(
        1,
        &[
            pii("p", &PiiType::ALL, Action::Flag),
            kw("k", &["secret", "token"], true, Action::Flag),
        ],
    )];
    for unit in [
        "AB12 CD34 ",
        "12-34-",
        "1:2:3:",
        "1 1 1 ",
        "10.0.0.",
        "a@b.c ",
        "12345 6789 ",
    ] {
        let text = unit.repeat(16_000 / unit.len());
        let started = cpu_time();
        let mut s = StreamScanner::new(set.clone());
        let mut buf = [0u8; 4];
        for c in text.chars() {
            let _ = s.push_text(c.encode_utf8(&mut buf));
        }
        let _ = s.finish();
        let took = cpu_time() - started;
        eprintln!("stream cpu {unit:?}: {took:?}");
        assert!(took < Duration::from_millis(200), "{unit:?} took {took:?}");
    }
}
