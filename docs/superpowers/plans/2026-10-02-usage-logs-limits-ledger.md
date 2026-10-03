# SDD ledger — plan: docs/superpowers/plans/2026-10-02-usage-logs-limits.md

Branch: v2. Plan commit: d8c2126. Baseline: cargo 683, ui 1491, E2E 56/4.
Ruling: team leads VIEW their team's limits/budgets; only admins change them (lead self-service LATER) — why: simplest reading of spec 8 'within limits an admin set' — cost if wrong: leads ask admins.
Ruling: plan 5 residual Minors (model add output/idempotent message; non-admin allowlist must not offer routes with no callable target; agreement test with missing owner) carried into Task 1 dispatch.
Preflight: tasks sequential in plan order; shared files proxy.rs (1,4,5,6,7), snapshot.rs (4,6), migrations numbered 0006-0009 in task order.
Task 1 dispatched (with task-1-carryover.md), BASE d8c2126.
Deploy (owner consent 'deploy plan 5', 2026-10-02 13:52): built d8c2126 (= plan 5 bbdba9b + docs) from a clean archive; production backup data-20261002-135212 + binary; service active; public /, /models, /routes 200; /v1/models without key 401.
Task 1 implemented: d03a78b (carry-over), a080e4b. ACCEPT: prices visible to non-admins (not secret; members see their spend). Task 1 review dispatched; Task 2 dispatched, BASE a080e4b.
Task 1 review: CRITICAL — src/logs/ git-ignored (.gitignore:27 'logs/'), commit a080e4b doesn't build clean; fix sent to the Task 2 implementer to do first. Important: retention ignores stop (slow shutdown). Minor: in-flight batch on shutdown untested; failed batch not retried. Fix round 1 (task-1-fix-round-1.md) after Task 2. Lesson for CONVENTIONS: gate includes building a clean archive (git archive HEAD) before reporting done.
Task 2 implemented: 8eb7be9 (tracks src/logs, anchors ignore to /logs/), 751da9f. Conventions: clean-archive gate added. Task 2 review dispatched; Task 1 fix round 1 dispatched (resume Task 1 implementer).
Task 2 review: PASS, Approved (scope holds; mutations killed; clean archive builds). Minors → task-3-additions.md. Task 2: complete (751da9f).
Task 1 fix round 1: d9e1779. Re-review dispatched. Task 3 dispatched (with task-3-additions.md), BASE d9e1779.
Task 1 re-review: all 3 ADDRESSED (cancel-safe DELETE probed). Minor: write_failures must appear on /metrics → Task 7 addition (uf_log_write_failures_total); retry-once count test & uninterruptible 200 ms sleep ACCEPT. Task 1: complete (d9e1779).
Task 3 implemented: af4ae7b (DONE). Task 3 review dispatched; Task 4 dispatched, BASE af4ae7b.
Task 3 review: PASS, Approved; Important perf (lead group=key/user/team correlated subquery 2-5 s on 300k rows) + 4 Minor. Fix round 1 (task-3-fix-round-1.md) after Task 4 commits — resume Task 3 implementer.
Task 4 implemented: 4836668 (751 Rust tests; burst pinned). Task 4 review dispatched; Task 3 fix round 1 dispatched (resume), BASE 4836668.
Task 3 fix round 1: 46f043d. Re-review dispatched. Task 5 dispatched, BASE 46f043d.
Task 4 review: PASS, Approved; 3 Minors → task-6-additions.md. Readings accepted (key team in subjects; estimate capped at limit; failed calls refund estimate; PUT replaces; longest wait). Task 4: complete (4836668).
Task 5 implemented: 7696561 (786 Rust tests). ACCEPT (known limit, README later): block budgets can overshoot by in-flight spend (~1 s writer batching). Task 5 review dispatched; Task 6 dispatched (with task-6-additions.md), BASE 7696561.
Task 3 re-review: all 5 ADDRESSED; logs list for a lead 436 s → 0.94 s on 300k rows (usage ~1 s either way, a range scan). Minor: optimize-failure untested (ACCEPT). Task 3: complete (46f043d).
Task 5 review: PASS w/ deviation; Important: Retry-After capped at 1 day for weekly/monthly; seed race under-blocks after a new budget. Rulings: Retry-After = true seconds to period end (no cap) — why: honest wait, clients back off — cost if wrong: very long hints; seed merges max. Fix round 1 (task-5-fix-round-1.md) after Task 6.
Task 6 implemented: a2aa3bc (Task 4 minors), 631841b (cache). Task 6 review dispatched; Task 5 fix round 1 dispatched (resume), BASE 631841b.
Task 6 review: CRITICAL — cached answer crosses teams on team/user/key id reuse (INTEGER PRIMARY KEY without AUTOINCREMENT; key holds only scope id). Important — route/provider edits don't invalidate. Ruling: one mechanism — config fingerprint in the snapshot; refresh clears the whole cache when it changes (task-6-fix-round-1.md) — why: covers deletes, id reuse, admin and CLI writes, no per-path purges to forget — cost if wrong: extra misses after admin changes. Fix round queued after Task 5's fix round (checkout busy).
Task 5 fix round 1: 3807765 (829 tests). Re-review dispatched. Task 6 fix round 1 dispatched (resume), BASE 3807765.
Task 5 re-review: all 6 ADDRESSED (probes reverted → tests fail). Long-call concern ACCEPT. Task 5: complete (3807765).
Task 6 fix round 1: 15dc23b (fingerprint clear; 835 tests; RED shown for id reuse, re-enable, base_url). Ruling: include the fingerprint in the CacheKey too, closing the in-flight store-after-clear race — sent to the implementer as a follow-up commit before re-review.
Task 6 follow-up: 7d11fe9 (fingerprint in key; in-flight race RED→green). Re-review of 15dc23b+7d11fe9 dispatched. Task 7 dispatched (with task-7-additions.md), BASE 7d11fe9.
Cache re-review: all ADDRESSED; both mechanisms pinned by tests; fingerprint stable on unrelated refreshes; no Critical/Important. Minor: any key create/revoke clears the whole cache (ACCEPT; README known limit in Task 10/final wave). Task 6: complete (7d11fe9).
Task 7 implemented: a21171f. Task 7 review dispatched; Task 8 dispatched, BASE a21171f.
Task 7 review: COMPLIANT; Important: /metrics distinguishable when off (JSON 404 vs SPA fallback). Fix round 1 (task-7-fix-round-1.md) after Task 8 commits — resume Task 7 implementer.
Task 8 implemented: 3b83e27 (1539 ui tests). Ruling: Errors only server-side via new errors=true filter (Task 7 fix round adds API; Task 9 switches console) — why: client-side filtering over pages misleads — cost: small API addition. Task 8 review dispatched; Task 7 fix round dispatched (resume).
Task 8 review: PASS, Approved (4 mutations killed). Minors → task-9-additions.md (loading-state h1 repo-wide; stale doc); tile links to /logs unfiltered ACCEPT. Task 8: complete (3b83e27).
Task 7 fix round 1: ac6f60a (no hand-back received; verified by re-review). Re-review dispatched. Task 9 dispatched (with task-9-additions.md), BASE ac6f60a.
Task 7 fix round 1 hand-back arrived late: confirms ac6f60a; RED shown for all items; the uncommitted logs.rs/admin.json edits it saw are the Task 9 implementer's doc-comment addition (expected).
Task 7 re-review: all 6 ADDRESSED (/metrics unset identical to unknown path except nonce). Minor PARKED (final wave): unreachable 404 branch in metrics serve. Console Errors-only switch owned by Task 9 (in progress). Task 7: complete (ac6f60a).
2026-10-03 21:57: session restart stopped Task 9 mid-work (d7e373e committed = additions; guards + lib/limits + lib/money uncommitted). Resumed.
Task 9 implemented: d7e373e (additions), bef109f (1713 ui tests; E2E 56/4 green on its head). Task 9 review dispatched; Task 10 dispatched, BASE bef109f.
Task 9 review: COMPLIANT; Important: editing a budget's period creates a second budget; missing session-over/typing tests on new forms. Fix round 1 (task-9-fix-round-1.md) after Task 10 commits.
Task 10 implemented: e1c9aae (E2E 64/4 x3; binary 23.06 MB; JS 234.7 kB gz). Task 10 review dispatched; Task 9 fix round 1 dispatched (resume).
Task 10 review: PARTIAL — cache.spec only tests user scope (keys without team_id); minors. Sent to the Task 9 fix-round implementer as additions. Task 10: complete after that round's re-review.
Task 9+10 fix round: 1087147, 580525b (1727 ui; E2E 64/4 x3; team-scope cache mutation fails as it should). Re-review + FINAL REVIEW (uf-final-reviewer, review-d8c2126..580525b.diff) dispatched in parallel.
Task 9+10 re-review: all ADDRESSED. Tasks 9 and 10: complete (580525b). All plan 6 tasks complete; final review running.
Final review (Opus): ready after fixes. Important: reused user/team ids inherit logs/usage/spend; README stale; owner's other teams charged by a team key. Rulings in final-fix-wave.md: null-on-delete triggers; key's team only (team-less key → owner's teams); estimate charge on 499/error streams (marked estimated); members see no team/gateway spend. Final fix wave dispatched, BASE 580525b.
Plan 6 final fix wave: 73ade50..ced90cf (cargo 869, ui 1731, E2E 64/4 x3). Scoped re-review dispatched.
Final fix wave re-review: APPROVED, items 1-8 ADDRESSED (mutations fail). Parked Minors: M1/M2 rebuild counts deleted-team rows via user membership (team semantics, restart-only); M3 README Develop data dir note; Logs errors=true includes 499 while usage errors excludes it (LATER). Plan 6 COMPLETE at ced90cf.
