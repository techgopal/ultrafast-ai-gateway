---
name: uf-workflow
description: How plans are executed in the ultrafast-ai-gateway repo — which project agents do which role, the ledger, review packages, batching, gates and the one-fix-wave rule. Use whenever executing or resuming a plan in this repository, alongside superpowers:subagent-driven-development.
---

# Running a plan in ultrafast-ai-gateway

This adapts superpowers:subagent-driven-development (SDD) to this repository.
SDD's process holds; these rules decide the details.

## Roles and models

| Role | Agent (`.claude/agents/`) | Model |
|---|---|---|
| Controller (plans, rulings, ledger, dispatch) | main session | Opus |
| Implementer, fix rounds, fix wave | `uf-implementer` | Sonnet |
| Task review, scoped re-review | `uf-reviewer` | Sonnet |
| Final whole-branch review | `uf-final-reviewer` | Opus |

Dispatch with `subagent_type` set to the agent; do not pass `model` (the agent
file decides). Escalate a single task to an Opus implementer only after two
fix rounds fail to converge, and ledger why.

## Workspace and ledger

- Workspace: `bash <sdd>/scripts/sdd-workspace PLAN` → `.superpowers/sdd/<plan>/`
  (git-ignored). Briefs: `bash <sdd>/scripts/task-brief PLAN N`. Review
  packages: `bash <sdd>/scripts/review-package PLAN BASE HEAD`.
- `progress.md` is the recovery map: every dispatch, commit range, verdict and
  `Ruling: <what> — <why> — <cost if wrong>` goes there. After compaction trust
  it and `git log`, not memory.
- At plan end copy it to `docs/superpowers/plans/<plan>-ledger.md`, commit, then
  delete the workspace. Anything a later plan needs (patterns, rules) goes into
  `docs/CONVENTIONS.md` before the workspace is deleted.

## Dispatches

- A dispatch names: where the task fits (one line), the brief path, decisions
  of earlier tasks the brief cannot know, rulings on ambiguities, the report path.
  Conventions are in `docs/CONVENTIONS.md`; do not paste them.
- Batch small same-shape items into one dispatch.
- One implementer in the checkout at a time. A reviewer of the previous task may
  run in parallel (it works in its own copy).
- Interruptions (usage limits, restarts): check `git status`, `pgrep -a ultrafast`
  and the report file, then resume the agent with SendMessage stating the exact
  state; re-dispatch fresh only if resuming fails.

## Reviews and fixes

- Every task gets a task review. Fix rounds: resume the implementer; each round
  ends with a scoped re-review. Minor findings may be parked (ledger) into a
  later hardening batch instead of a round of their own.
- Final review once per plan (`uf-final-reviewer`), then ONE fix wave with the
  complete list, then ONE scoped re-review; residuals are parked with rulings.

## Stops

Run continuously; rule on ambiguities and ledger them. Stop and ask the owner
only for: pushing, merging or deploying; destructive operations; anything
touching the production service; a plan so broken every path is a guess.
Owner decisions already made are in memory (model split, client architecture,
run mode, team-role rules) — follow them.
