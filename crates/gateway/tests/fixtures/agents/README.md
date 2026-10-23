# Recorded agent requests

Request bodies sent by the coding agents, recorded against a local recorder
(`scripts/agent-recorder.py`, started by `scripts/record-agents.sh`) and
trimmed with `scripts/trim-agent-fixture.py`.

| File | Agent | Request |
| --- | --- | --- |
| `claude-code-2.1.305.json` | Claude Code (recorded with 2.1.306) | `POST /v1/messages?beta=true`, `--model claude-opus-5-5`, one prompt |
| `codex-0.160.1.json` | Codex 0.160.1 | `POST /v1/responses`, model `gpt-5.5`, one prompt |

Trimming keeps the shape and drops everything that could identify a person or
a machine:

- every `text`, `instructions` and tool output string becomes `t`;
- tool and schema descriptions are cut to their first line (80 characters);
  the grammar of Codex's `apply_patch` is cut to 80 characters;
- `metadata.user_id` (Claude Code) becomes `u`;
- every UUID (session, thread, turn, installation, prompt cache key) becomes a
  numbered placeholder `00000000-0000-0000-0000-00000000000N`;
- headers are not kept; tests set the headers they need. The headers an agent
  sends are listed in the design: `anthropic-beta`, `anthropic-version`,
  `x-claude-code-session-id`, `x-stainless-*` (Claude Code); `session-id`,
  `thread-id`, `x-codex-*`, `originator` (Codex).

The script fails if a home path, e-mail address, user name or host name is
left in the result. Refresh the files with `scripts/record-agents.sh` when an
agent changes.
