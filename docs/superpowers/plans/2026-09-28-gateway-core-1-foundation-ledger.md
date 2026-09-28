# Gateway core plan 1: execution ledger

Record of the run: decisions made during execution (lines starting `Ruling:`), findings deferred to follow-up work (`minor (deferred)` / `follow-up`), and the commits for each task.

```

Branch: v2. Merge base with main: b7fa3be. Plan commit: f48783c.
All subagents use model "fable" (user instruction).
Skill dir: /home/gopal/.claude/plugins/cache/claude-plugins-official/superpowers/6.4.1/skills/subagent-driven-development

## Pre-flight scan

| Tasks | Produces vs consumes | Finding |
|---|---|---|
| 1 -> all | crate names ultrafast-translate / ultrafast-gateway, workspace deps | consistent |
| 2 -> 4,5 | types::*, TranslateError | Role has no Tool variant; T4/T5 role matches cover 3 variants. consistent |
| 2 -> 8,9 | parse_request, render_response, render_error, render_stream_event/error | signatures match use in proxy.rs |
| 3 -> 4,5 | SseParser::feed -> Vec<SseEvent> | matches StreamDecoder use |
| 4 -> 5 | provider/mod.rs matches extended by T5 | T5 edits T4 file; additive. consistent |
| 4,5 -> 8 | ProviderKind::parse, Target, build_request, parse_response | match |
| 6 -> 7,8,10 | generate_key, hash_key, Cipher | match |
| 7 -> 8,10 | Store API | match |
| 8 -> 9 | stream_response signature, now_secs pub(crate) | T8 ships 501 stub; T9 replaces |
| 8 -> 10 | AppState, router, DEFAULT_MAX_BODY_BYTES | match |
| 1 vs 10 | CI: T1 drops docker step, T10 re-adds | consistent |

| Task | Self-consistency |
|---|---|
| 1 | ok. `git tag v1-final main` is local only (no push). |
| 2 | ok. 7 tests. |
| 3 | ok. 5 tests. |
| 4 | ok. 10 tests. |
| 5 | ok. 6 tests; T5 total 28. |
| 6 | ok. 3 tests. |
| 7 | ok. 3 tests. |
| 8 | stream_response is a 501 stub: staged delivery, replaced in T9. |
| 9 | ok. Expected count 21. |
| 10 | Dockerfile pins rust:1.82; local toolchain is 1.92, deps may need newer than 1.82. |

Ruling: T8 501 stub stands — plan stages streaming into T9 with its own tests; not a placeholder left in the final branch — cost if wrong: none, T9 replaces it.
Ruling: commits use `git -c user.name=techgopal -c user.email=techgopal2@gmail.com` (repo history identity; machine has no git identity) — cost if wrong: author on commits needs rewriting.
Ruling: if rust 1.82 cannot build the dependency set, raise `rust-version` and the Docker image tag to the lowest version that builds, and record it — spec only says "pinned to a Rust version" — cost if wrong: MSRV higher than needed.
Ruling: plan code is uncompiled; implementers may make the smallest change needed to compile/pass while keeping the brief's interfaces and tests, and must list each deviation in their report — cost if wrong: drift from plan, caught by task review.

## Progress
Task 1: implemented (base f48783c, head b71d885); concerns: MSRV 1.82 unverified; release.yml may reference v1 layout (deferred to final review)
Task 1: verified by controller: tag v1-final == main (b7fa3be); trailer correct.
Task 1: minor (deferred): rust-version 1.82 unverified (no 1.82 toolchain run); CI uses stable only
Task 1: minor (deferred): ci.yml cache has no restore-keys
Task 1: minor (deferred): release.yml untouched, may reference v1 layout
Task 1: note for user: implementer deleted the untracked v1 Cargo.lock from disk and regenerated a v2-only one
Task 1: complete (commits f48783c..b71d885, review clean)
Task 2: dispatching (base b71d885)
Task 2: implemented (base b71d885, head 2b3bc39); implementer concern: parse_request silently ignores tool_choice/functions/response_format/n/tool_calls
Ruling: plan defect — spec s5 says inexpressible content is an error. parse_request must reject (Unsupported) any request field that changes output and that ChatRequest cannot carry: tool_choice, functions, function_call, response_format, n>1, logit_bias, assistant tool_calls/function_call, and a message with null/missing content. Fields that do not change the text output (user, metadata, stream_options, seed, presence/frequency penalties are output-affecting => reject) — cost if wrong: callers sending these fields get 400 until plan 6 adds support.
Task 2: pre-review fix applied per ruling (head 70968db); review dispatched
Task 2: verified by controller: author + trailer on both commits
Task 2: minor (deferred): usage total uses unchecked u32 add (openai.rs render_response/render_stream_event) — use saturating_add
Task 2: minor (deferred): usage JSON block duplicated in two renderers
Task 2: minor (deferred): text part without string `text` returns Unsupported, should be InvalidRequest
Task 2: minor (deferred): `tools` rejection wording differs from "field '...'" form
Task 2: minor (deferred): "stream": null rejected as InvalidRequest
Task 2: minor (deferred): reject list is a denylist; unlisted output-affecting fields (web_search_options, verbosity, message refusal) still ignored — decide allowlist vs denylist
Task 2: minor (deferred): Done chunk carries usage with non-empty choices regardless of stream_options.include_usage
Task 2: minor (deferred): no test for n: 1.0
Task 2: complete (commits b71d885..70968db, review clean)
Task 3: dispatching (base 70968db)
Task 3: implemented (base 70968db, head eff37ff); deviation: find_boundary replaced by line scan handling \n, \r\n, \r mixes (brief's code failed on \r\r and \n\r\n); 3 tests added
Task 3: minor (deferred): SseParser buffer unbounded if upstream never sends a blank line — add a max event size
Task 3: minor (deferred): no finish(); trailing event without blank line at end of stream is dropped
Task 3: minor (deferred): feed rescans partial event from start (quadratic on huge events)
Task 3: minor (deferred): from_utf8_lossy silently replaces invalid bytes in a complete event
Task 3: minor (deferred): event: value trimmed fully; event-only blocks emitted with empty data
Task 3: minor (deferred): no tests for invalid UTF-8 or empty blocks under split feeding
Task 3: complete (commits 70968db..eff37ff, review clean)
Task 4: dispatching (base eff37ff)
Task 4: implemented (base eff37ff, head 4ed9e23); amendment: saturating token counts, tool_calls => Unsupported, refusal => ContentFilter; review dispatched
Task 4: minor (deferred): function_call rejection untested
Task 4: minor (deferred): content+refusal ordering differs between response and stream paths
Task 4: minor (carried to Task 5): SSE event with empty data aborts stream as Malformed — skip in StreamDecoder::feed
Task 4: minor (deferred): in-stream error only recognized when error.message is a string
Task 4: minor (deferred): in-stream errors hardcoded 502/non-retryable (decide in plan 3 retry work)
Task 4: minor (carried to Task 5): Target/HttpRequest derive Debug and would print credentials — redact
Task 4: minor (deferred): partial usage object overwrites stored count with None; non-string delta.content skipped silently; empty-string refusal forces ContentFilter
Task 4: complete (commits eff37ff..4ed9e23, review clean)
Ruling: Task 5 amended to also (a) give Target and HttpRequest redacting Debug impls, (b) skip SSE events with empty data in StreamDecoder::feed — both are in mod.rs which Task 5 edits anyway; credentials-never-logged is a global constraint — cost if wrong: small extra scope in Task 5.
Task 5: dispatching (base 4ed9e23)
Task 5: implemented (base 4ed9e23, head 8cb0ee6); 50 tests; concern: Message.name dropped for Anthropic
Ruling: anthropic::build returns Unsupported when a message has a name — spec s5, inexpressible content is an error — cost if wrong: callers using `name` with Anthropic get 400 instead of a response that ignores it.
Task 5: minor (deferred): thinking blocks discarded; Anthropic role-ordering rule not validated (provider returns 400)
Task 5: pre-review fix applied (head 0f59452); review dispatched
Task 5: minor (deferred): Debug test asserts contains("23") loosely
Task 5: minor (deferred): anthropic text block with missing text treated as empty; unknown stop reasons map to None
Task 5: minor (deferred): Debug redaction covers only authorization/x-api-key; url printed in full (revisit when Gemini/Azure added)
Task 5: minor (deferred): thinking blocks discarded silently — tension with "never silently dropped"; needs human decision
Task 5: complete (commits 4ed9e23..0f59452, review clean)
Task 6: dispatching (base 0f59452)
Task 6: implemented (base 0f59452, head bd02afd); amendment: redacting Debug, reject all-zero master key; review dispatched
Task 6: minor (deferred): key material not zeroized on drop (needs zeroize dependency)
Task 6: verified by controller: author + trailer
Task 6: minor (deferred): error-echo test covers only non-hex path; no near-zero key acceptance test
Task 6: minor (deferred): 96-bit random nonce; XChaCha20 would change stored format — decide before data exists
Task 6: minor (deferred): from_hex trims silently; public items lack doc comments
Task 6: complete (commits 0f59452..bd02afd, review clean)
Task 7: dispatching (base bd02afd)
Task 7: implemented (base bd02afd, head fc067c6); deviation: sqlx `macros` feature added; amendment: ProviderRow Debug redaction, expires_at format check; review dispatched
Task 7: minor (deferred): expiry check accepts impossible dates like 02-31; revoke_key on unknown id returns Ok
Task 7: minor (deferred): open_in_memory pool can reap its single connection (idle_timeout/max_lifetime) and lose the DB
Task 7: minor (deferred): provider_by_name does not filter org_id (fine for single org)
Task 7: minor (deferred): revoke_key does not check rows_affected; re-revoke overwrites revoked_at
Task 7: minor (deferred): no positive boundary test for expires_at
Task 7: complete (commits bd02afd..fc067c6, review clean)
Task 8: dispatching (base fc067c6)
Task 8: implemented (base fc067c6, head 2faa0e5); 73 tests; amendment items 4 and 6 applied; extra: provider 3xx => 502; review dispatched
Task 8: minor (deferred): provider response body read without size cap
Task 8: minor (deferred): every body rejection reported as 413
Task 8: minor (deferred): timeouts untested
Task 8: minor (deferred): request body is buffered (up to limit) before auth runs; comment overstates
Task 8: minor (carried to Task 9): provider 401/403 passed through as-is; caller reads it as a bad gateway key
Task 8: minor (deferred): test gaps — undecryptable credential, unknown provider kind, `p/` and `/m` model forms, oversized body with bad key
Task 8: complete (commits fc067c6..2faa0e5, review clean)
Ruling: provider 401 and 403 become 502 with a message that the provider rejected the gateway's credential for that provider; other provider 4xx still pass through — a caller must be able to tell "my key is bad" from "the gateway's provider credential is bad" — cost if wrong: callers relying on provider 401/403 passthrough see 502.
Task 9: dispatching (base 2faa0e5)
Task 9: implemented (base 2faa0e5, head 2ad1ceb); concern: StreamDecoder::feed discards events decoded before an error in the same chunk; implementer worked around it by line-splitting in the gateway
Ruling: fix at source — StreamDecoder::feed returns events decoded before an error and stores the error (new take_error()); gateway workaround removed; Task 9 file list extended to translate/provider/mod.rs — spec s5 puts stream handling in translate and clients will share it — cost if wrong: extra API surface (take_error) on StreamDecoder.
Task 9: minor (deferred): streams are cut at 300 s by the shared client total timeout — needs a separate streaming timeout policy (plan 3)
Task 9: pre-review fix applied (head 6f2a56f); review dispatched
Task 9: minor (deferred): mid-stream provider errors forward the provider's own message (incl. auth errors) — decide masking policy
Task 9: minor (deferred): lost-connection reqwest error not logged in stream path
Task 9: minor (deferred): duplicated upstream fixture in stream tests; no gateway test for lost-connection branch or Anthropic mid-stream error
Task 9: complete (commits 2faa0e5..6f2a56f, review clean)
Task 10: dispatching (base 6f2a56f)
Task 10: implemented (base 6f2a56f, head 91eff72); 98 tests; rust-version raised 1.82 -> 1.88 (idna_adapter needs edition2024; lockfile max rust-version is 1.88); release.yml disabled; review dispatched
Task 10: minor (deferred): serve handles ctrl-c only, not SIGTERM (docker stop not graceful)
Task 10: minor (deferred): gateway.db created 0644; only master.key is 0600
Task 10: review: approved with 1 Important (plan-mandated): no SIGTERM handling, docker stop hangs to kill timeout
Ruling: fix SIGTERM now (shutdown_signal in app.rs) — the image is a deliverable and its main stop path must work — cost if wrong: none material.
Ruling: gateway.db and side files made 0600, data dir 0700 (restrict_permissions in config.rs) — db holds encrypted credentials and key hashes — cost if wrong: another local user/process that needed read access loses it.
Task 10: minor (deferred): args validated after master key/db are created; master.key create race and no sync_all; empty api key stored; key name not validated
Task 10: minor (deferred): "whitespace" message also covers control chars; test couples to words "pass"/"token"
Task 10: minor (deferred): CI builds release twice and image uncached; CI does not test declared MSRV 1.88; 1.85-1.87 untried
Task 10: minor (deferred): README v1 text below new section contradicts v2 (Rust 1.75 badge); Dockerfile has no HEALTHCHECK; deployment/build.sh and docker-compose.yml may reference deleted deployment/Dockerfile
Task 10: fix round 1/5 dispatched (fix base 91eff72)
Task 10: fix round 1 implemented (head 0153b17); note: one anonymous docker volume left behind by container test; re-review dispatched
Task 10: correction: fix round 1 head is ba3bd99 (implementer reported a wrong SHA 0153b17); SHAs in implementer handbacks are unreliable, always read git log
Task 10: re-review of fix round 1 failed on usage limit; re-dispatching (fix base 91eff72, head ba3bd99)
Task 10: fix round 1/5 (2 addressed, 0 open — SIGTERM shutdown, db file permissions; commits 91eff72..ba3bd99)
Task 10: minor (deferred): db file has umask perms during Store::open in a pre-existing loose dir; 0700 applied to created parent dirs too; set_permissions failure aborts startup
Task 10: minor (deferred): with UF_MASTER_KEY set and data dir missing, startup fails ("could not open the database") because the dir is only created on the key-file path
Task 10: complete (commits 6f2a56f..ba3bd99, review clean after 1 fix round)
All tasks complete. Final whole-branch review dispatching (base f48783c, head ba3bd99).
Final review (head ba3bd99): ready to merge WITH FIXES. 0 critical, 6 important. Reviewer ran tests (101 pass), clippy, fmt: clean. Agrees with all rulings; notes T2 and T8 rulings were incompletely enforced.
Ruling: request parsing switches from denylist to allowlist (final finding 6) — spec s5 "never silently dropped"; reviewer and task-2 reviewer both flagged the denylist — cost if wrong: clients sending any unlisted field (e.g. parallel_tool_calls, top_k) get 400 until support is added.
Ruling: in-stream credential errors are masked like pre-stream 401/403; decoders report status 401 for auth-type in-stream errors — one masking policy in errors.rs — cost if wrong: callers lose provider detail for auth failures mid-stream.
Ruling: caps — provider non-stream response 32 MiB, SSE event 1 MiB — no spec value exists; chosen as generous defaults — cost if wrong: a legitimate very large response is rejected with 502.
Final review: ONE fix dispatch (fix base ba3bd99); findings in final-findings.md
Final review: tracked follow-ups (not fixed on this branch): 300 s stream cut/timeout policy; in-stream error retryability; thinking blocks decision; XChaCha/nonce decision; zeroize; open_in_memory reaping; revoke_key rows_affected; impossible dates; SSE finish() and lossy UTF-8; partial usage overwrite; URL in Debug; arg validation order, master.key race, empty API key; db umask window; MSRV in CI; HEALTHCHECK; stale deployment README; test gaps; anthropic system-message hoisting and DEFAULT_MAX_TOKENS 4096.
Final review: fix wave implemented (commits ba3bd99..8b67e17); 135 tests reported; new dep http-body-util; scoped re-review dispatched
Final review: fix wave re-review (ba3bd99..8b67e17): all 6 findings + 2 small items ADDRESSED; reviewer ran fmt, clippy, tests: clean, 135 passed. No new Critical/Important.
Final review: minor (follow-up): duplicate JSON key (unsupported value then null) passes the allowlist
Final review: minor (follow-up): Cargo.lock format now version 4; unused pub fn stream_response can be removed
Final review: minor (follow-up): in-stream bad-key errors under codes other than invalid_api_key are not masked; other provider 4xx messages pass through
```
