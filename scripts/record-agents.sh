#!/usr/bin/env bash
# Runs the installed Claude Code and Codex through a throwaway gateway, for
# both families of model, against a fake provider that records what it gets.
#
#   scripts/record-agents.sh [path-to-ufaigw-binary]
#
# Four calls are made:
#   claude -> a Claude model        (Anthropic provider: passed through)
#   claude -> an OpenAI-family name (OpenAI provider: translated)
#   codex  -> an OpenAI-family name (OpenAI provider: passed through)
#   codex  -> a Claude model        (Anthropic provider: translated)
# The outcome of each is printed. The requests the fake provider received are
# written, untrimmed, to a directory outside the repository (printed at the
# end): trim one into a fixture with scripts/trim-agent-fixture.py. They hold
# paths and ids of this machine: do not commit them as they are.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(dirname "$here")"
binary="${1:-$repo/target/release/ufaigw}"
[ -x "$binary" ] || { echo "no binary at $binary: cargo build --release -p ufaigw-gateway" >&2; exit 1; }
for tool in python3 claude codex; do
  command -v "$tool" >/dev/null || { echo "$tool is not installed" >&2; exit 1; }
done

work="$(mktemp -d "${TMPDIR:-/tmp}/uf-agents-XXXXXX")"
rec="$work/recordings"; mkdir -p "$rec" "$work/data" "$work/codex"
pids=()
cleanup() { for p in "${pids[@]:-}"; do kill "$p" 2>/dev/null || true; done; }
trap cleanup EXIT

free_port() { python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'; }

# The fake provider.
python3 "$here/agent-recorder.py" 0 "$rec" >"$work/recorder.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 50); do
  provider_port="$(sed -n 's/^listening //p' "$work/recorder.log" | head -1)"
  [ -n "$provider_port" ] && break; sleep 0.1
done
[ -n "${provider_port:-}" ] || { echo "the recorder did not start" >&2; exit 1; }

# The gateway.
port="$(free_port)"
admin_pass="record-$(date +%s)-pw"
UF_DATA_DIR="$work/data" UF_HOST=127.0.0.1 UF_PORT="$port" UF_INSECURE_COOKIES=true \
  UF_ADMIN_EMAIL=agents@example.test UF_ADMIN_PASSWORD="$admin_pass" \
  UF_MCP_CATALOG=off RUST_LOG=warn NO_COLOR=1 \
  "$binary" serve >"$work/gateway.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 100); do
  curl -fs "http://127.0.0.1:$port/health" >/dev/null 2>&1 && break; sleep 0.1
done
curl -fs "http://127.0.0.1:$port/health" >/dev/null || { cat "$work/gateway.log" >&2; exit 1; }

# Two providers on the fake one, a model and a route for each name.
key="$(PORT="$port" PROVIDER="http://127.0.0.1:$provider_port" PASS="$admin_pass" python3 - <<'PY'
import json, os, urllib.request, http.cookiejar
base = f"http://127.0.0.1:{os.environ['PORT']}"
jar = http.cookiejar.CookieJar()
opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
csrf = ""
def call(method, path, data=None):
    req = urllib.request.Request(base + path, method=method,
        data=None if data is None else json.dumps(data).encode(),
        headers={"content-type": "application/json", "x-csrf-token": csrf})
    with opener.open(req) as r:
        body = r.read()
        return json.loads(body) if body else None
csrf = call("POST", "/api/auth/login", {"email": "agents@example.test", "password": os.environ["PASS"]})["csrf_token"]
prov = os.environ["PROVIDER"]
a = call("POST", "/api/providers", {"name": "anthropic-fake", "kind": "anthropic", "base_url": prov, "api_key": "k"})["id"]
o = call("POST", "/api/providers", {"name": "openai-fake", "kind": "openai", "base_url": prov, "api_key": "k"})["id"]
def route(name, provider_id, model):
    m = call("POST", "/api/models", {"provider_id": provider_id, "name": model})["id"]
    call("PATCH", f"/api/models/{m}", {"enabled": True})
    call("PUT", f"/api/models/{m}/grants", {"everyone": True, "team_ids": [], "user_ids": []})
    call("POST", "/api/routes", {"name": name, "primaries": [{"model_id": m, "weight": 1}],
        "retries": 0, "first_token_timeout_ms": 30000, "total_timeout_ms": 60000,
        "breaker_failures": 5, "breaker_window_s": 60, "breaker_open_s": 30,
        "everyone": True, "team_ids": [], "fallbacks": []})
route("claude-opus-5-5", a, "claude-opus-5-5")
route("gpt-5.5", o, "gpt-5.5")
print(call("POST", "/api/keys", {"name": "agents"})["secret"])
PY
)"

gateway="http://127.0.0.1:$port"
cat >"$work/codex/config.toml" <<TOML
model_provider = "gateway"
[model_providers.gateway]
name = "gateway"
base_url = "$gateway/v1"
env_key = "UF_AGENT_KEY"
wire_api = "responses"
TOML

outcome() { # label, exit code, count of recordings that grew
  printf '%-34s %s\n' "$1" "$2"
}
run_claude() {
  local model="$1"
  ANTHROPIC_BASE_URL="$gateway" ANTHROPIC_AUTH_TOKEN="$key" CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1 \
    CLAUDE_CONFIG_DIR="$work/claude" DISABLE_TELEMETRY=1 \
    timeout 120 claude -p "say ok" --model "$model" >"$work/claude-$model.out" 2>&1 && echo "answered: $(tail -n 1 "$work/claude-$model.out" | head -c 60)" || echo "failed (see $work/claude-$model.out)"
}
run_codex() {
  local model="$1"
  UF_AGENT_KEY="$key" CODEX_HOME="$work/codex" \
    timeout 120 codex exec --skip-git-repo-check -m "$model" "say ok" >"$work/codex-$model.out" 2>&1 && echo "answered: $(tail -c 60 "$work/codex-$model.out" | tr '\n' ' ')" || echo "failed (see $work/codex-$model.out)"
}

echo "Claude Code -> claude-opus-5-5 (Anthropic, passed through)"; outcome "  result" "$(run_claude claude-opus-5-5)"
echo "Claude Code -> gpt-5.5 (OpenAI, translated)";              outcome "  result" "$(run_claude gpt-5.5)"
echo "Codex -> gpt-5.5 (OpenAI, passed through)";                outcome "  result" "$(run_codex gpt-5.5)"
echo "Codex -> claude-opus-5-5 (Anthropic, translated)";         outcome "  result" "$(run_codex claude-opus-5-5)"

echo
echo "The fake provider received $(ls "$rec" | wc -l) requests; untrimmed, in:"
echo "  $rec"
echo "Outputs and logs are in $work (remove it when done)."
trap - EXIT; cleanup
