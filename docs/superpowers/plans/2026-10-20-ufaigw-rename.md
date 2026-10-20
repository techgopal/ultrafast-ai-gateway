# ufaigw Rename Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every name a person types, installs or pulls becomes `ufaigw`; "Ultrafast" stays the product's display name; release 3.0.0.

**Architecture:** A mechanical rename in five gated commits — Rust crates and binary first (everything builds on their names), then the image and release workflow (with one new startup check), then the Python clients, the TypeScript clients, and last docs, version and a whole-repo audit.

**Tech Stack:** Rust (cargo workspace, clap, sqlx), Docker, GitHub Actions, Python (maturin, pytest), TypeScript (pnpm, vitest), wasm-pack.

**Spec:** `docs/superpowers/specs/2026-10-20-ufaigw-rename-design.md`

## Global Constraints

- Command `ufaigw`; crates `ufaigw-gateway`, `ufaigw-translate`, `ufaigw-client`, `ufaigw-client-wasm`, `ufaigw-client-py`.
- Archives `ufaigw-vX.Y.Z-<target>.tar.gz` / `.zip`; image `ghcr.io/techgopal/ufaigw`.
- npm `@ufaigw/client`, `@ufaigw/admin`; PyPI `ufaigw`, `ufaigw-admin`; imports `ufaigw`, `ufaigw_admin`, `ufaigw._native`; console package `ufaigw-console`.
- Image user `ufaigw`, uid and gid **999** (the 2.1.1 image's `ultrafast` user is `uid=999 gid=999`), home and data `/var/lib/ufaigw`.
- User-Agent `ufaigw/<version>`; OTel default service name `ufaigw`; tracing targets `ufaigw::…`; download names `ufaigw-<stamp>.db`, `ufaigw-config-<stamp>.json`; alert payload `"gateway": "ufaigw <version>"`.
- Unchanged: `"format": "ultrafast-config"`, the MCP `_meta` key `"ultrafast"`, `"ultrafast-route"`, migrations, `UF_*` names, the repository name and its URLs, the console brand "Ultrafast", type names `UltrafastError`, release notes and CHANGELOG entries for 2.1.1 and earlier.
- No `ultrafast` alias of any kind.
- Version 3.0.0 everywhere `docs/RELEASING.md` lists.
- `docs/CONVENTIONS.md` applies: its gates, its commit identity, no assistant trailers, stage specific paths, never push, never touch the production instance (`ultrafast-gateway` service, port 3900).

## Review Focus

- A 2.1.x Docker user keeps `-v data:/var/lib/ultrafast`: the gateway refuses to start with the mount hint, rather than starting empty (Task 2 tests).
- A 2.1.x Docker user remounts at `/var/lib/ufaigw`: files owned by 999 are readable and writable by the new user (Task 2 step: uid check).
- A config file exported by 2.1.x (`"format": "ultrafast-config"`) imports into 3.0.0 (Task 1 keeps the existing import test green unchanged).
- `UF_OTEL_SERVICE_NAME=ultrafast` still overrides the new default (Task 1 test).
- A fresh image with no database anywhere starts normally (Task 2 test).

---

### Task 1: Rust crates and the `ufaigw` binary

**Files:**
- Modify: `Cargo.toml` (workspace members' dependency names), `Cargo.lock` (regenerated), `crates/*/Cargo.toml` (package names; `[[bin]] name = "ufaigw"`; the `ufaigw-translate` dependency in `crates/client` and `crates/client-wasm`)
- Modify: every `ultrafast_gateway` / `ultrafast_translate` / `ultrafast_client` path in `crates/**` (sources, tests, benches, build scripts)
- Modify: `crates/gateway/src/main.rs` (clap `name = "ufaigw"`, `about` stays "Ultrafast AI gateway"; `UF_OTEL_SERVICE_NAME` default and `otel_service_name` → `"ufaigw"`; tracing target `ufaigw::setup`; argv vectors in tests → `"ufaigw"`)
- Modify: `crates/gateway/src/app.rs:505`, `alerts/deliver.rs:483`, `guardrails/external.rs:206` (User-Agent `concat!("ufaigw/", env!("CARGO_PKG_VERSION"))`), `alerts/mod.rs:45,97`, `api/backup.rs:125`, `api/config.rs:73`
- Modify: `env!("CARGO_BIN_EXE_ultrafast")` → `CARGO_BIN_EXE_ufaigw` in `crates/gateway/tests/backup.rs`, `cli_config.rs`
- Modify: `-p ultrafast-…` and `target/release/ultrafast` in `.github/workflows/ci.yml`, `release.yml:159-161`, `publish.yml` (cargo lines and the `check` sed), `Dockerfile:22` (build line only; the rest is Task 2), `clients/admin-py/tests/gateway.py:85,91` (cargo `-p` and binary path only), `crates/gateway/tests/release_workflow.rs` expectations
- Do NOT change: `access.rs:302` (`ultrafast-route`), `mcp/discover.rs:184` (`_meta` key), the config `format` string, `clients/admin-py/tests/gateway.py:32` (`DEPLOYED` names the production instance)

**Interfaces:**
- Produces: binary `target/<profile>/ufaigw`; crate names above; Rust paths `ufaigw_gateway::…`, `ufaigw_translate::…`, `ufaigw_client::…`.

- [ ] **Step 1: Write the failing tests**

In `crates/gateway/tests/cli_config.rs`, a test `the_command_is_ufaigw`: run `env!("CARGO_BIN_EXE_ufaigw")` with `--version`; assert success and that stdout starts with `"ufaigw "` (the version moves to 3.0.0 in Task 5, so do not assert it here).
In `crates/gateway/src/alerts/mod.rs` tests: change the assertion at line 97 to `starts_with("ufaigw ")`.
In the `main.rs` tests that parse the CLI: a test `otel_service_name_defaults_to_ufaigw_and_the_setting_overrides_it` — parsing `["ufaigw", "serve"]` gives `"ufaigw"`; with `UF_OTEL_SERVICE_NAME=ultrafast` (or `--otel-service-name ultrafast` if the flag exists; use the existing pattern in that test module for env) gives `"ultrafast"`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultrafast-gateway --test cli_config the_command_is_ufaigw` — Expected: compile error, `CARGO_BIN_EXE_ufaigw` not defined.

- [ ] **Step 3: Rename**

Package names, `[[bin]]`, then `sed`-style path renames across `crates/`, then the listed literals. `cargo update -w` to rewrite `Cargo.lock` workspace entries without changing any dependency version (check `git diff Cargo.lock` touches only the five workspace crates).

- [ ] **Step 4: Verify**

Run the Rust gates from CONVENTIONS (`cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all`). Expected: all pass, including the unchanged config-import tests.
Run: `git grep -nE "ultrafast[-_](gateway|translate|client)|CARGO_BIN_EXE_ultrafast|-p ultrafast|release/ultrafast" -- ':!docs/release-notes' ':!CHANGELOG.md' ':!docs/superpowers' ':!docs/CONVENTIONS.md' ':!clients/admin-py/tests/gateway.py'` — Expected: only lines in `clients/` package metadata and imports that Tasks 3–4 own (list them in the report).

- [ ] **Step 5: Commit** — `refactor: the crates and the command are ufaigw`

### Task 2: The image, the release archives, and the moved-volume check

**Files:**
- Modify: `Dockerfile:27-35` — `groupadd --system --gid 999 ufaigw && useradd --system --uid 999 --gid 999 --create-home --home-dir /var/lib/ufaigw ufaigw`; binary copied to `/usr/local/bin/ufaigw`; `USER ufaigw`; `UF_DATA_DIR=/var/lib/ufaigw`; `VOLUME /var/lib/ufaigw`; `ENTRYPOINT ["ufaigw"]`
- Modify: `.github/workflows/release.yml:164-167` (`exe=ufaigw`, `ufaigw.exe`, `name="ufaigw-${ref}-…"`), `:251` (`images: ghcr.io/${{ github.repository_owner }}/ufaigw`); `.github/workflows/ci.yml:61-63` (`ufaigw:ci`), `:111` (`/CN=ufaigw test CA`)
- Modify: `crates/gateway/src/config.rs` (new function and constants), `crates/gateway/src/main.rs` (call it before the SQLite store opens for `serve`)
- Test: `crates/gateway/src/config.rs` tests; `crates/gateway/tests/release_workflow.rs`

**Interfaces:**
- Produces: `pub const IMAGE_DATA_DIR: &str = "/var/lib/ufaigw";`, `pub const OLD_IMAGE_DATA_DIR: &str = "/var/lib/ultrafast";`, `pub fn check_moved_image_volume(data_dir: &Path, image_dir: &Path, old_image_dir: &Path) -> anyhow::Result<()>`.

- [ ] **Step 1: Write the failing tests** (in `config.rs` tests, tempdirs standing in for both image paths)

- `a_volume_left_at_the_old_mount_point_refuses_to_start`: `image_dir` empty, `old_image_dir/gateway.db` exists, `data_dir == image_dir` → `Err` whose message contains `image_dir.display()`, `old_image_dir.display()` and `"mount the volume at"`.
- `a_volume_at_the_new_mount_point_starts`: `image_dir/gateway.db` exists, old one exists too → `Ok`.
- `a_fresh_image_starts`: neither database exists → `Ok`.
- `a_data_dir_elsewhere_is_not_checked`: `data_dir` is a third tempdir, old database exists → `Ok`.

In `release_workflow.rs`: expectations for `ufaigw-${ref}-`, `exe=ufaigw`, and the image name.

- [ ] **Step 2: Run them to verify they fail** — `cargo test -p ufaigw-gateway --lib config::tests::a_volume` → compile error, function not defined.

- [ ] **Step 3: Implement `check_moved_image_volume`**

Error copy, one line: `"the database is at {old}, the 2.1 image's data directory; this image keeps its data in {new}: mount the volume at {new} (for example -v ultrafast-data:{new})"`. Call it in `main.rs` for `serve` on SQLite with `(&cli.data_dir, Path::new(IMAGE_DATA_DIR), Path::new(OLD_IMAGE_DATA_DIR))`, before `Store::open`.

- [ ] **Step 4: Verify**

Rust gates. Then `docker build -t ufaigw:plan .` and:
- `docker run --rm ufaigw:plan --version` → `ufaigw …`
- `docker run --rm --entrypoint id ufaigw:plan` → `uid=999(ufaigw) gid=999(ufaigw)`
- Old mount point: `docker run --rm -v ufaigw-plan:/var/lib/ultrafast ghcr.io/techgopal/ultrafast-ai-gateway:2.1.1 serve` long enough to create its database (`-e UF_PORT=0`, stop it after it logs that it is listening), then `docker run --rm -v ufaigw-plan:/var/lib/ultrafast -e UF_PORT=0 ufaigw:plan serve` → exits non-zero with the mount hint.
- New mount point, same volume: `docker run -d --name ufaigw-plan -v ufaigw-plan:/var/lib/ufaigw -e UF_PORT=0 ufaigw:plan serve` → starts, logs listening, its database is the 2.1.1 one (no setup prompt in the log); stop it by name.
- Remove the container, volume and image afterwards. Never use port 3900.

- [ ] **Step 5: Commit** — `build(image): ufaigw image and archives; refuse to start on a volume left at the 2.1 mount point`

### Task 3: Python clients

**Files:**
- Move: `crates/client-py/python/ultrafast/` → `crates/client-py/python/ufaigw/`; `clients/admin-py/ultrafast_admin/` → `clients/admin-py/ufaigw_admin/`
- Modify: `crates/client-py/pyproject.toml` (`name = "ufaigw"`, `module-name = "ufaigw._native"`), `crates/client-py/src/lib.rs` (`#[pymodule]` registration if it names the package path), `clients/admin-py/pyproject.toml` (`name = "ufaigw-admin"`, package path), `clients/admin-py/gen.sh`, `generator.yml` (package name), every `import ultrafast` / `from ultrafast…` / `ultrafast_admin` in sources, tests, READMEs, `.github/workflows/clients.yml`
- Keep: `UltrafastError` and other `Ultrafast*` type names; `test_binary.py:23` (`"ultrafast-config"`)

**Interfaces:**
- Consumes: Task 1's binary path for the admin tests.
- Produces: `import ufaigw`, `ufaigw.Client`, `ufaigw.AsyncClient`, `ufaigw._native`, `import ufaigw_admin`.

- [ ] **Step 1: Failing test** — in `crates/client-py` tests, `test_the_package_is_ufaigw`: `import ufaigw; assert ufaigw.Client and ufaigw._native`. In `clients/admin-py/tests`, `test_the_package_is_ufaigw_admin`: `import ufaigw_admin`.
- [ ] **Step 2: Run** — the test commands `clients.yml` uses for each package → `ModuleNotFoundError: No module named 'ufaigw'`.
- [ ] **Step 3: Rename** (move with `git mv`; regenerate the admin client with `gen.sh` and check the diff is names only).
- [ ] **Step 4: Verify** — both packages' full test runs as in `clients.yml`; `git grep -nE "\bultrafast(_admin)?\b" crates/client-py clients/admin-py` shows only `UltrafastError`-style type names, `ultrafast-config`, and `DEPLOYED`.
- [ ] **Step 5: Commit** — `refactor(clients-py): the packages are ufaigw and ufaigw-admin`

### Task 4: TypeScript and WebAssembly clients, console package

**Files:**
- Modify: `clients/ts/package.json`, `clients/admin-ts/package.json` (`@ufaigw/client`, `@ufaigw/admin`), their lockfiles (`pnpm install --lockfile-only`), sources/tests/READMEs importing `@ultrafast/*` or `ultrafast_client_wasm`, wasm-pack output name (`ufaigw_client_wasm.js`), `ui/package.json` (`ufaigw-console`), `.github/workflows/publish.yml` and `clients.yml` package names
- Keep: `UltrafastError`-style type names; the console's "Ultrafast" brand text

**Interfaces:**
- Consumes: Task 1's crate name `ufaigw-client-wasm`.
- Produces: `import { … } from "@ufaigw/client"`, `"@ufaigw/admin"`.

- [ ] **Step 1: Failing test** — in `clients/ts/test`, `the package is named @ufaigw/client`: read `package.json`, assert `name === "@ufaigw/client"`; same for admin-ts.
- [ ] **Step 2: Run** — `pnpm --dir clients/ts test` → fails on the name.
- [ ] **Step 3: Rename.**
- [ ] **Step 4: Verify** — each package's lint, typecheck, test, build as `clients.yml` runs them; the console gates from CONVENTIONS; `pnpm publish --dry-run` in each client shows the new name.
- [ ] **Step 5: Commit** — `refactor(clients-ts): the packages are @ufaigw/client and @ufaigw/admin`

### Task 5: Docs, version 3.0.0, and the audit

**Files:**
- Modify: version to `3.0.0` in every file `docs/RELEASING.md` lists, plus `ui/package.json`; `Cargo.lock`
- Modify: `README.md` (commands, archive and image names, Docker example `docker run -d --name ufaigw -p 3000:3000 -v ufaigw-data:/var/lib/ufaigw ghcr.io/techgopal/ufaigw:3.0.0`, data path in the settings table; title and prose keep "Ultrafast"), `docs/RELEASING.md` (package names, trusted-publisher names), `CONTRIBUTING.md`, `SECURITY.md`, `.github/ISSUE_TEMPLATE/*`, `docs/compose/postgres.yml`, `ui/README.md`, `docs/brand` alt text, `.gitignore`/`.gitleaks.toml` entries that name old paths
- Modify: `docs/CONVENTIONS.md` — the production-instance paragraph is unchanged; `pgrep -a ultrafast` becomes `pgrep -a 'ultrafast|ufaigw'` with "test gateways are `ufaigw`; the production process keeps its name until the owner redeploys it"
- Create: `docs/release-notes/v3.0.0.md` (title "Ultrafast 3.0.0", the rename table, and the spec's upgrade notes verbatim in substance: binary, Docker remount with the uid note, `UF_OTEL_SERVICE_NAME=ultrafast`, no migration); `CHANGELOG.md` 3.0.0 entry

- [ ] **Step 1: Failing test** — `release_workflow.rs` (or the existing version-consistency test, if one exists) asserts every listed version file says `3.0.0`; run it → fails.
- [ ] **Step 2: Bump versions and edit docs.**
- [ ] **Step 3: Audit** — `git grep -ni "ultrafast"` over the whole repo; every remaining line must be one of: product name in prose/UI/titles, repository URL or name, stored-format identifiers, `Ultrafast*` type names, the production instance in CONVENTIONS and `DEPLOYED`, history (release notes ≤ 2.1.1, CHANGELOG ≤ 2.1.1, `docs/superpowers`). Paste the categorised remainder into the report.
- [ ] **Step 4: Verify** — all gates (Rust, console, E2E per CONVENTIONS), the clients' test runs, and the `git archive HEAD` clean-build check.
- [ ] **Step 5: Commit** — `release: 3.0.0 — the command, image and packages are ufaigw`
