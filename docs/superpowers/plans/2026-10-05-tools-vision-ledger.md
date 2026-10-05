# Plan 10 progress — tools, vision, cache single-flight

Plan: docs/superpowers/plans/2026-10-05-tools-vision.md · Branch: plan-10 (from main 82a3b15)
Run mode: SDD, back-to-back; stops only for push/merge/deploy.

## Rulings
- Own-spend visibility dropped from plan 10: already present (usage/overview/budgets scoped by role); README wording fixed in Task 10 — owner was told.

## Log
- T1 dispatched (uf-implementer) from 82a3b15
- T1 DONE 82a3b15..33d23f2 (699 tests). Review dispatched; T2 dispatched.
- T1 review: PASS/Approved, no Critical/Important. Minors parked to hardening batch (with T6): M1 tool_call_id on non-tool msg → InvalidRequest; M2 base64 padding only at end; M3 tool parameters must be object, description string; M4 client-wasm reject role tool until T9 (T9 adds tools anyway — fold in T9). M5 ToolCallStart not in estimate — ok.
  Ruling: park minors — none affects later interfaces — cost if wrong: small lax-validation window inside the branch only.
- T2 DONE_WITH_CONCERNS 33d23f2..aad9339 (1071 tests). Fixed a proxy.rs assertion T1 broke (T1 report/review missed it). Added tracing dep to translate. Review + T3 dispatched.
- T3 DONE aad9339..fc7ac5a (1082 tests). Ruling (accepted): text-only multi-part messages sent as one joined string, array only with images — keeps bodies identical and compat providers that reject arrays working — cost if wrong: part boundaries lost (no semantic effect). Review + T4 dispatched.
- T2 review: PASS/Approved. Minors parked to hardening batch: A1 accept tool type "custom" (Anthropic client tool); A2 pin empty-answer empty-text-block in StreamRenderer; A3 proxy.rs:162 pin exact message; A4 unknown-index tool delta dropped silently (note only).
- T4 DONE fc7ac5a..6d13728 (1092 tests). Review + T5 dispatched.
- T3 review: PASS/Approved, 1 Important: I1 compat provider sending parallel whole calls each with index 0 + new id merges calls → fix round (after T5 leaves the checkout, before T6). Minors to hardening: O2 out-of-order starts; O3 stream non-array tool_calls silently ignored vs Malformed; O5 test gaps (image+tool_calls, tool msg with name, user text after tool_result round trip).
  Ruling (revises T1 uniform refusal): with tools empty/absent, tool_choice auto|none and parallel_tool_calls are ignored (they change nothing); required or a named tool → InvalidRequest. — SDKs send tool_choice with tools: [] — cost if wrong: a meaningless field accepted. Do in hardening batch across all adapters.
- T5 DONE 6d13728..6a7cbfc (1101 tests). T5 review dispatched; T3 fix round 1 (I1 + empty-tools ruling) dispatched to T3 implementer.
- T4 review: PASS/Approved. Minors to hardening: P1 streamed no-arg tool_use emits no delta → OpenAI callers get "" (emit "{}" at content_block_stop when no delta came); P2 merge adjacent user messages (Tool after non-assistant) for Anthropic.
- T5 review: Needs fixes. Important G1: repeated call_0 ids across turns → tool result named after the earliest call; fix = latest match. Fix round 1 queued for T5 implementer after T3 fix round leaves checkout.
  Ruling G3: send tool schemas to Gemini as `parametersJsonSchema` (full JSON Schema, e.g. additionalProperties) instead of `parameters` (OpenAPI subset) — OpenAI-style schemas would otherwise 400 on Gemini — cost if wrong: older Gemini-compatible endpoints ignore/reject the field.
  G2 (duplicated Stop→ToolCalls rule) folded into the same fix round.
- T3 fix round 1 DONE 6a7cbfc..eaa4830 (1105 tests; shared check_tool_choice). Note: 2 gateway config permission tests flaked once under load — watch.
- T5 fix round + hardening H1–H8 dispatched to T5 implementer. Then ONE scoped re-review of 6a7cbfc..HEAD covering T3 FR1 + T5 FR1 + hardening.
- T5 FR1+hardening DONE eaa4830..c6b90ea (1112 tests). Scoped re-review 6a7cbfc..c6b90ea + T6 dispatched.
- Fix-rounds re-review: all fixed, mutations killed, 1112 green. parametersJsonSchema verified by controller (documented, mutually exclusive with `parameters`; symfony/ai, litellm adopted it). Residual minors parked to final fix wave: R1 base64 length mod 4; R2 gemini test pin Auto-without-tools accepted; R3 README note on tool_choice with empty tools (Task 10).
- T6 DONE_WITH_CONCERNS c6b90ea..76f1b32 (no gateway bug found; openapi regenerated). Review + T7 dispatched.
- T7 DONE_WITH_CONCERNS 76f1b32..2d9fd79 (1131 tests). Controller fix round 1 before review: rename counter to uf_cache_flight_waits_total; waiters of a failed leader must call concurrently (a waiter that misses after waiting drops its guard) — cost if wrong: outage serializes waiters × timeout.
- T7 FR1 DONE 2d9fd79..8adb4a3. Review of 76f1b32..8adb4a3 + T8 dispatched.
- T6 review: PASS/Approved. S1 content nullable/optional, S2 tool_choice oneOf, S3 csrf code assert → sent to T8 implementer (affects TS types). S5 (tool request temp>0 not cached test) parked to final wave.
- T6 review: PASS/Approved. S1 content nullable/optional, S2 tool_choice oneOf, S3 csrf code assert → sent to T8 implementer (affects TS types). S5 (tool request temp>0 not cached test) parked to final wave.
- Ops: /tmp quota full (stale reviewer scratch r4 8.4G etc.) — deleted stale scratch dirs. Reviewers must delete target dirs.
- T8 DONE_WITH_CONCERNS 8adb4a3..f58aa04 (vitest 1973, e2e 84). Includes T6 S1–S3. Review + T9 dispatched.
- T7 review: PASS/Approved. Minors to final wave: F1 waits>=1 lower bound can flake (raise delay); F2 add leader-panic unit test + leader-abort integration test; F3 README note (Task 10).
- T8 review: Needs fixes — 3 Important (tools JSON error hidden in collapsed section; image budget must be whole-request ~9 MiB incl. history; truncated stream keeps broken tool_calls and wedges the thread) + 8 Minor. Fix round queued for T8 implementer after T9 leaves checkout (one implementer at a time). Include minors 4–10 in the same round.
- T9 DONE f58aa04..bcdc8bc (cargo 1142, pytest 132, ts 212). Review + T8 fix round dispatched.
- T9 review: PASS, 1 Important (Python: passing reply.tool_calls back fails json.dumps). Fix round queued after T8 FR1: C1 accept ToolCall/{id,name,arguments} in assistant dicts; C3 Python tools accept flat {name, description, parameters} like other clients (keep OpenAI shape too); C5 Message dataclass gains tool_calls/tool_call_id, docstring role tool. C2/C6/C7/C8 parked (C2 existing pattern; ruling: keep).
- T8 FR1 DONE bcdc8bc..6dc828d (vitest 1984, e2e 84). Lost OpenAPI descriptions on tools/tool_calls (schema_with) — parked to final wave. Re-review + T9 FR1 dispatched.
- T8 FR1 re-review: Approved, all fixed. N1–N3 (budget recheck/IMAGES_HISTORY tests, text-only budget) parked to final wave.
- T9 FR1 DONE 6dc828d..a7a6467 (pytest 135). Re-review + T10 dispatched.
- T9 FR1 re-review: all addressed, no regressions; 2 minor notes (flat dict with extra 'type' key; TypeError mapping) — no action.
- T10 DONE a7a6467..f8b1b74 (all gates, e2e x3, clean archive). Final review dispatched over 82a3b15..f8b1b74.
- FINAL REVIEW: ready after fix wave. 5 Important (I1 anthropic renderer interleaved parallel tool deltas into closed blocks; I2 tool msg `name` refused on anthropic/gemini; I3 `strict` dropped; I4 gemini ids repeat per turn → duplicate ids on failover; I5 gemini 3 needs thought signatures) + M1–M14.
  Rulings: I1 = first tool call streams live, later calls buffered and emitted whole (start, one delta, stop) in index order after earlier blocks close at Done; never interleave blocks. I4 = id `call_<8 hex of sha256(responseId)>_<n>`, random 8 hex if no responseId. I5 VERIFIED (Google documents placeholder; many gateways hit the 400): send thoughtSignature "skip_thought_signature_validator" on every history functionCall part; document. M7, M14 LATER (M14 in Known limits). All else fix now.
- Fix wave dispatched (fresh uf-implementer).
- Fix wave DONE f8b1b74..be47f40 (cargo 1162, vitest 1988, e2e 84, ts 213, pytest 136). New dep sha2 in translate. Scoped re-review dispatched.
- Fix-wave re-review: READY TO MERGE (1162 green; I1–I5, M-items fixed, mutations killed). Residual R2 doc wording fixed by controller. Residual: wasm ToolIn.strict has no Rust test (TS covers) — parked.
