# Gateway core plan 2: execution ledger

Record of the run: decisions made during execution (lines starting `Ruling:`), findings deferred to follow-up work (`minor (deferred)`, `follow-up`, `parked`), questions left for the project owner (`decision for human`), and the commits for each task.

```

Branch: v2. Plan commit / base: 6696774. Baseline: 135 tests pass.
All subagents use model "fable" (user instruction).
Skill dir: /home/gopal/.claude/plugins/cache/claude-plugins-official/superpowers/6.4.1/skills/subagent-driven-development
Lesson from plan 1: commit SHAs in implementer handbacks are unreliable; always read `git log`.

## Pre-flight scan

| Tasks | Produces vs consumes | Finding |
|---|---|---|
| 1 -> 3,4,8 | store/ layout, DEFAULT_ORG, check_timestamp, now/after, KeyRow fields, revoke_key -> bool | consistent; plan-1 callers of revoke_key ignore the value (tests use .unwrap()) |
| 2 -> 3,5,6 | Role, TeamRole, UserStatus, Principal, password fns | consistent |
| 3 -> 4,6,7,8 | Tx, StoreError::Duplicate, user/team/audit methods, generate_secret | consistent; Store::count_active_admins and Tx::count_active_admins both exist by design |
| 4 -> 6,8 | sessions/tokens store API | consistent |
| 5 -> 6,7,8,10 | Action, Decision, authorize, list_scope | checked against role table rows 1-33: consistent |
| 6 -> 7,8,9,10 | ApiError, Authed, require, AppState::new (sync) | Task 9 changes AppState::new to async+fallible: intended, plan says so |
| 8 -> 9 | one test half ignored until Task 9 | intended |
| 9 -> 10 | none | - |
| 6,7,8 -> 10 | 32 routes (7 + 14 + 11); 28 secured operations vs role table 33 rows | table covers all secured operations |

| Task | Self-consistency |
|---|---|
| 1 | plan1_database_migrates relies on restricting sqlx Migrator to version 1; plan gives the method |
| 2 | ok |
| 3 | ok |
| 4 | ok |
| 5 | DeleteUser self => Forbidden in policy for non-admins; admin self-delete handled by handler rule (409) — consistent |
| 6 | per-address login limit (20) could be hit by test suites sharing 127.0.0.1 only through failures within one app instance; each test builds its own app — ok |
| 7 | the_last_admin_is_protected accepts either of two codes for self-delete — deliberate |
| 8 | ok |
| 9 | an_expiring_key test sleeps 3 s — acceptable |
| 10 | committed_spec_is_current + CI diff both guard the spec — intended duplication |

Ruling: commits use `git -c user.name=techgopal -c user.email=techgopal2@gmail.com` — same as plan 1 — cost if wrong: author needs rewriting.
Ruling: implementers write the implementation from interfaces, rules and tests (plan's stated method); they may adapt signatures only where the compiler forces it and must report each deviation — cost if wrong: drift, caught by task review.

## Progress
Task 1: dispatching (base 6696774)
Task 1: implemented (base 6696774, head b55daf1); 145 tests; review dispatched
Task 1: minor (deferred): in_memory_database_survives_idle cannot detect reaping (no time passes); rule verified by reading
Task 1: minor (deferred): year 0000 accepted by check_timestamp; test-only raw queries lack org_id filter
Task 1: complete (commits 6696774..b55daf1, review clean)
Task 2: dispatching (base b55daf1)
Task 2: implemented (base b55daf1, head bd2f522); 156 tests; review dispatched
Task 2: minor (deferred): email rule accepts consecutive dots in domain; DUMMY_HASH init uses expect
Task 2: minor (carried to Task 6): force DUMMY_HASH at startup so the first unknown-account sign-in is not slower and the expect cannot fire on a request
Task 2: minor (deferred): verify_password trusts cost parameters in the stored hash without a ceiling
Task 2: complete (commits b55daf1..bd2f522, review clean)
Task 3: dispatching (base bd2f522)
Task 3: implemented (base bd2f522, head 87c6539); 178 tests; review dispatched
Task 3: note: Tx holds the only connection of the in-memory pool; callers must read via Store first, then begin, write via Tx, commit/drop before using Store again. Later tasks may add Tx read methods they need.
Task 3: note: store does not normalize emails; callers pass normalize_email output
Task 3: minor (deferred): put_member / insert_invite do not check the org of referenced rows (matters with multi-org)
Task 3: minor (deferred): insert_invite duplicate gives plain error; missing org-scope tests for 5 methods; users.rs is 603 lines
Task 3: controller check for Tasks 6-8: no /api handler may use the audit-free Store::insert_key / insert_provider / revoke_key wrappers; each handler calls tx.audit before commit
Task 3: complete (commits bd2f522..87c6539, review clean)
Task 4: dispatching (base 87c6539)
Task 4: implemented (base 87c6539, head c36b386); 195 tests; review dispatched
Task 5: dispatching while Task 4 is in review (base c36b386) — Task 5 touches only identity/policy.rs and identity/mod.rs, no overlap with Task 4's files; reviewer works from the diff file
Task 4: minor (deferred): delete_sessions_of SQL duplicated on Store and Tx; touch_token silent on missing id; live_token has no length check; token column test less strict than session one
Task 4: note: user deletion relies on ON DELETE CASCADE for sessions/tokens — keep hard delete
Task 4: complete (commits 87c6539..c36b386, review clean)
Task 5: implemented (base c36b386, head 74b7d05); 199 tests; policy_matrix 141 cases; review dispatched
Task 5: note: CreateKey for another user in a led team is Allow by policy; membership of the owner in that team is enforced by the handler (Task 8 rule 1) — controller to verify in Task 8
Task 5: note: interpreted cell — ViewUser shares_led_team=true from a caller who leads nothing => Hidden
Task 5: note: last-admin protection lives in handlers (Task 7 rule 4), policy allows admin everything
Task 6: dispatching while Task 5 is in review (base 74b7d05) — no file overlap (Task 6: api/, identity/limiter.rs, app.rs, main.rs, tests/)
Task 5: minor (deferred): Principal with duplicate team entries would be judged inconsistently (led_teams vs team_role) — extractor builds teams from team_members whose primary key (team_id,user_id) prevents duplicates; add a pinning test later
Task 5: decision for human: a lead may remove or (via admin-only PutMember Lead) not demote a co-lead; RemoveMember by a lead can remove another lead or themselves — per table
Task 5: controller check for Task 8: handler must call authorize before looking up the team in CreateKey; owner membership check in handler
Task 5: complete (commits c36b386..74b7d05, review clean)
Task 6: implemented (base 74b7d05, head dd2bb65); 250 tests; review dispatched
Task 6: deviations: Tx::create_session and Tx::delete_other_sessions_of added to store/sessions.rs; policy reuse ViewUser{self}/UpdateUser{self} for me/logout/password; touch_token gated on token last_used_at; password change answers 429 when limiter blocks
Task 6: minor (deferred): concurrent POST /api/setup on a file db can answer 500 to the loser (never two admins)
Task 6: minor (deferred): bootstrap fails when only one of UF_ADMIN_* is set even if users exist; bootstrap admin named "Admin"
Task 6: minor (deferred): limiter per-address limit is unusable behind a reverse proxy; not yet in README
Task 6: review: needs fixes — 2 Important: (1) sign-in limit bypass by parallel requests (check and record not atomic, hashing unbounded); (2) logout writes without Tx/audit
Ruling: limiter gets atomic try_begin (attempt counted before verification, forgiven on success) and hashing is bounded by a 4-permit semaphore — the limit must hold under concurrency — cost if wrong: a burst of legitimate concurrent sign-ins for one email could be limited early.
Ruling: logout is audited (auth.logout) in a transaction — global constraint says every state-changing /api call is audited; brief rule 13 omitted it by mistake — cost if wrong: extra audit volume.
Ruling: invites can be accepted only by users in status `invited`; any other status answers 404 — closes password reset of an active user via a leftover invite — cost if wrong: no invite-based password reset for active users (none was specified).
Task 6: minor (deferred): rule 4 touch throttling has no integration test; logout by a member untested; limiter prune scans whole map; dedicated self-service policy actions (me/logout/password) should replace reuse of ViewUser/UpdateUser
Task 6: minor (deferred): five failed sign-ins by anyone also block that user's password change for 15 min
Task 6: fix round 1/5 dispatched (fix base dd2bb65)
Task 6: fix round 1 implemented (head 176c1b0; implementer first reported a wrong SHA); 258 tests; re-review dispatched
Task 6: minor (deferred): >5 simultaneous correct sign-ins for one email get 429 beyond the fifth; attempts ending in 500 stay counted; logout of an already-ended session writes no audit entry; bootstrap uses its own semaphore
Task 7: dispatching while Task 6 fix is in re-review (base 176c1b0) — Task 7 adds api/users.rs, api/teams.rs, api/audit.rs, tests; touches api/mod.rs only to register routes
Task 6: fix round 1/5 (3 addressed, 0 open — limiter race, logout audit, invite status; commits dd2bb65..176c1b0)
Task 6: minor (deferred): hashing semaphore permit is held by the handler future, not the blocking task — a dropped request frees the permit while Argon2 still runs (use acquire_owned moved into spawn_blocking)
Task 6: minor (deferred): change_password calls attempt_succeeded before the transaction, login after commit — inconsistent
Task 6: complete (commits 74b7d05..176c1b0, review clean after 1 fix round)
Task 7: implemented (base 176c1b0, head 803c2d5); 305 tests; review dispatched
Task 7: decision for human: PUT team member lets a lead tell an existing user id from a missing one (204/409 vs 404); leads also have no way to find a user outside their team except by id — consider adding members by email
Task 7: minor (deferred): user_name duplicates trimmed_name in api/auth.rs; PATCH with unchanged role/status counts as a change; empty PATCH is 400; no-op writes skip audit; disabling a user deletes their unused invites
Ruling: commit trailer now also carries the Claude-Session line (harness attribution changed mid-run) — cost if wrong: none.
Task 8: dispatching while Task 7 is in review (base 803c2d5) — Task 8 adds api/keys.rs, api/providers.rs, api/tokens.rs, tests; touches api/mod.rs only to register routes
Task 7: review: needs fixes — 1 Important: POST /api/users/{id}/invite answers 403 for an existing id and 404 for a missing one to non-admins (existence leak); caused by amendment 3's load-then-require order
Ruling: where the policy's answer for the caller does not depend on the target (InviteUser, and any action that is Forbidden for every non-admin), call `require` BEFORE loading the target; load-then-require applies only to actions whose decision needs target facts — rule 1 "existence is never revealed" outranks my amendment's ordering — cost if wrong: none.
Task 7: minor (deferred): misplaced doc comment on list_users_in_teams/touch_user; test placed above use lines; `raw` test helper duplicated in two test files; SQLITE_BUSY surfaces as 500 under concurrent writers; a disabled invited user can only be deleted
Task 7: fix round 1/5 QUEUED until Task 8's implementer finishes (both would build and commit in the same checkout; Task 8's half-written files would break the fix's test run)
Task 8: implemented (base 803c2d5, head 618ab52); tests pass per implementer; review dispatched
Task 8: minor (deferred): active_key_by_hash now runs a joined SELECT (goes away in Task 9); API keys stored untrimmed; CLI changes not run for real; secret_name duplicates user_name
Task 7: fix round 1/5 dispatched now that Task 8's implementer is done (fix base 618ab52)
Task 8: review: needs fixes — 1 Important (against the later ruling only): provider PATCH/DELETE load the target before require(ManageProviders); no leak today (providers listable by all) but test locks the wrong order in
Task 8: minor (deferred): tokens revoke loads before require (no behaviour change); lead naming a led team can tell missing/inactive owner from non-member (422 field differs); shared helpers live in keys.rs; untrimmed API key; report test evidence summarized
Task 8: fix round 1/5 QUEUED until Task 7's fix is committed (same checkout)
Task 7: fix round 1 implemented (head 0cd4f91, read from git); 351 tests; re-review dispatched
Task 8: fix round 1/5 dispatched (fix base 0cd4f91)
Task 7: fix round 1/5 (1 addressed, 0 open — reinvite existence leak; commits 618ab52..0cd4f91); per-handler table confirmed in full by re-reviewer
Task 7: minor (deferred): response timing not equalized between hidden and missing; tests compare all headers byte for byte (flaky if a per-request header is added)
Task 7: complete (commits 176c1b0..803c2d5 + fix 0cd4f91, review clean after 1 fix round)
Task 8: fix round 1 implemented (head f772fcf, read from git); re-review dispatched
Task 9: dispatching while Task 8 fix is in re-review (base f772fcf)
Task 8: fix round 1/5 (1 addressed, 0 open — provider require-before-load; plus tokens order and key trimming; commits 0cd4f91..f772fcf)
Task 8: minor (deferred): CLI key trim untested; a non-admin sending a malformed body gets the body-parse error rather than 403 (body extractor runs before the handler; leaks nothing about ids)
Task 8: complete (commits 803c2d5..618ab52 + fix f772fcf, review clean after 1 fix round)
Task 9: implementer interrupted by usage limit after writing tests/snapshot.rs only (no src changes, head f772fcf); resumed the same implementer
Task 9: implemented (base f772fcf, head a4f26ce, read from git); 370 tests; review dispatched
Task 9: deviations: misconfigured provider now 404 on /v1 instead of 500 (brief rule 2); Store::pool() made pub; files outside list touched (lib.rs, store/keys.rs, store/mod.rs, api/mod.rs, api/auth.rs, three test files); three plan-1 tests gained a refresh call
Task 9: minor (deferred): SnapKey.team_id can be stale for up to 30 s after a team delete (nothing reads it yet)
Task 9: review: needs fixes — reviewer ran tests (370 pass) and clippy (clean). 1 Critical (plan-mandated): deleting a disabled/invited user makes their keys valid again (user_id set NULL => key treated as ownerless). 3 Important: revoke retry skips refresh; dropped request skips refresh silently; failed-refresh path untested.
Ruling: deleting a user who is not `active` revokes their keys in the same transaction; deleting an `active` user still leaves keys working and ownerless (owner-approved choice) — closes the disable-then-delete hole without changing the approved behaviour — cost if wrong: keys of an invited/disabled user cannot be kept alive across deletion.
Ruling: handlers refresh on no-op/already-done paths too, and refresh runs in a spawned task so a dropped request cannot skip it — access must not stay open silently — cost if wrong: a few extra refreshes.
Task 9: minor (deferred): concurrency test may pass without the mutex; misconfigured provider is logged on every refresh (noisy); expired keys stay in snapshot until revoked
Task 9: fix round 1/5 dispatched (fix base a4f26ce)
Task 9: fix round 1 implemented (head c89caba, read from git); 379 tests; re-review dispatched
Task 9: minor (deferred): retried user delete (404) and retried accept-invite (404) do not refresh; covered by the 30 s background refresh
Task 10: dispatching while Task 9 fix is in re-review (base c89caba)
Task 9: fix round 1/5 (4 addressed, 0 open — disabled-user delete revokes keys, refresh on no-op paths, refresh survives dropped caller, failed-refresh test; commits a4f26ce..c89caba)
Task 9: minor (deferred): refresh_snapshot is pub for a test; user-update no-op refresh untested; concurrent delete vs status change untested; a malformed provider id from an admin triggers a reload
Task 9: minor (deferred): if a disable's refresh failed and the admin then deletes the user, the key stays live in the stale snapshot until the next successful refresh (<= 30 s once refreshes work)
Task 9: complete (commits f772fcf..a4f26ce + fix c89caba, review clean after 1 fix round)
Task 10: implemented (base c89caba, head 73e5bd4, read from git); 390 tests; role table 165 cells passed unchanged; review dispatched
Ruling: the x-csrf-token header parameter is declared only on operations that need a caller, not on the three public POSTs (setup, login, accept-invite) — they never read it, and documenting it there would mislead client authors — cost if wrong: none.
Task 10: minor (deferred): response bodies built with json! are described by doc-only types in openapi.rs and can drift from the handlers
Task 10: minor (deferred): not run on Rust 1.88 (no toolchain installed); dtolnay/rust-toolchain@1.88 ref unverified until CI runs
Task 10: review: approved with 1 Important — reviewer ran tests (green), clippy (clean), spec diff (empty); role table matches brief cell for cell and cannot pass vacuously; 5 cells traced by hand. Important: coverage tests compare against the spec, not the router, so an undocumented route escapes the role table.
Ruling: router and spec are built from one registration (utoipa-axum OpenApiRouter) — the role table's guarantee must hold for routes nobody documented — cost if wrong: one more dependency; router construction changes shape.
Ruling: role table also asserts, for 200/201 object bodies, that top-level keys equal the documented response schema's properties — pins the doc-only wrapper types — cost if wrong: none.
Task 10: minor (deferred): role table 2xx cells check status only (list scoping covered elsewhere); Row.template and path closure are separate; unreachable! arm in main.rs; CSRF breakage is caught by api_auth tests, not the role table
Task 10: fix round 1/5 dispatched (fix base 73e5bd4)
Task 10: fix round 1 implemented (head 19bd75e, read from git); 391 tests; openapi/admin.json byte-identical after the change; re-review dispatched
Final whole-branch review dispatched in parallel with the Task 10 re-review (base 6696774, head 19bd75e); both are read-only
Task 10: fix round 1/5 (2 addressed, 0 open — router and spec from one registration; response keys pinned; commits 73e5bd4..19bd75e)
Task 10: parked — residual: a route can still be served without being in the spec by calling OpenApiRouter::route / route_service, by adding to the axum router after split_for_parts, or by a second registration in app.rs; no test enumerates the router — Ruling: real but needs a deliberate call to a different method, not an omission; defer a source-level guard test or clippy disallowed-methods rule to follow-ups; the doc comment at api/mod.rs:44-46 overstates and should be corrected in the final fix wave — cost if wrong: a deliberately undocumented route escapes the role table.
Task 10: minor (deferred): response key check is top-level only; api_roles comment assumes no public handler answers 404 to an empty request; report wrongly says there is no explicit /api/ route (there is, in app.rs)
Task 10: complete (commits c89caba..73e5bd4 + fix 19bd75e, review clean after 1 fix round, 1 parked)
All tasks complete. Waiting on the final whole-branch review.
Final review (head 19bd75e): ready to merge WITH FIXES. 0 critical. Reviewer ran tests (0 failures), clippy, fmt, OpenAPI diff: clean. Agrees with all rulings, one reservation (audit should say how many keys an active user's deletion left working).
Final review: fix before merge: (1) hashing permit moved into blocking task; (2) expired sessions deleted periodically. Controller adds to the same fix wave: name helpers merged; delete audit wording; overstated doc comment; KeyView.status `suspended`; header comparisons in tests.
Ruling: key status gains `suspended` for a key whose owner is not active — the console must not show a dead key as active — cost if wrong: API consumers must handle a fourth status value.
Final review: ONE fix dispatch (fix base 19bd75e); findings in final-findings.md
Final review: tracked follow-ups: trusted-proxy setting for the sign-in limiter (needed before production behind TLS proxy); BEGIN IMMEDIATE for Tx + concurrent-writer test on a file database; keys keeping their team after the owner leaves it / team delete (decide before plan 4 budgets); dedicated self-service policy actions; doc-only response types; Argon2 parameter ceiling; org checks in put_member/insert_invite; disabled invited user can only be deleted; stale SnapKey.team_id; expired keys in snapshot; Rust 1.88 CI run; lead's validation message distinguishing missing owner from non-member; guard against undocumented routes; email lockout by 5 failures (document)
Final review: decision for human: leads adding members by id — reviewer recommends adding by email or restricting leads to users they can already see, because a lead can add ANY user id to their team, then see that user's email and create keys in their name. Acceptable for merge, not for multi-team production.
Final review: fix wave implemented (commits 19bd75e..f9e52fd, 7 commits); 394 tests reported; scoped re-review dispatched
Final review: fix wave re-review (19bd75e..f9e52fd): all 7 findings ADDRESSED; reviewer ran tests (394 passed, 0 failed, 0 ignored), clippy, fmt, OpenAPI diff: all clean. No new Critical/Important.
Final review: minor (follow-up): test helper `compared` compares bodies through from_utf8_lossy, not raw bytes; session clean-up shares the refresher loop; a key with a dangling user_id would show active; team_name keeps its own helper
```
