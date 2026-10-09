#!/usr/bin/env bash
# Regenerates ultrafast_admin/_generated/ from ../../openapi/admin.json.
# The generator and the formatter it runs are pinned; the venv lives under
# ~/.cache, never in the repository. Running it twice changes nothing.
set -euo pipefail

GENERATOR_VERSION=0.29.1
RUFF_VERSION=0.16.10

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
venv="${UF_ADMIN_GEN_VENV:-$HOME/.cache/uf-admin-py/gen-venv}"
spec="$here/../../openapi/admin.json"

if ! "$venv/bin/openapi-python-client" --version 2>/dev/null | grep -q "$GENERATOR_VERSION"; then
  python3 -m venv "$venv"
  "$venv/bin/pip" install --quiet "openapi-python-client==$GENERATOR_VERSION" "ruff==$RUFF_VERSION"
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/uf-admin-gen.XXXXXX")"
trap 'rm -rf "$work"' EXIT

python3 "$here/prepare_spec.py" "$spec" "$work/admin.json"

RUFF_NO_CACHE=true PATH="$venv/bin:$PATH" "$venv/bin/openapi-python-client" generate \
  --path "$work/admin.json" \
  --config "$here/generator.yml" \
  --meta none \
  --output-path "$work/out" \
  --overwrite

rm -rf "$here/ultrafast_admin/_generated"
mkdir -p "$here/ultrafast_admin"
rm -rf "$work/out/.ruff_cache"
mv "$work/out" "$here/ultrafast_admin/_generated"
