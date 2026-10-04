# Contributing

Thanks for helping. Ultrafast is a small, one-maintainer project, so short and
focused changes are the easiest to review.

## Setup

You need Rust (the version in `rust-toolchain.toml`, installed by `rustup`),
Node 22 and pnpm. The binary embeds the console, so build the console first
for a full build:

```bash
pnpm --dir ui install --frozen-lockfile
pnpm --dir ui build
cargo build -p ultrafast-gateway
```

A plain `cargo build` works without Node; the console page then says it was
not built. See the Development section of the [README](README.md) for running
the gateway and the console in dev mode.

## Tests

Run what you changed before you open a pull request:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
pnpm --dir ui lint && pnpm --dir ui typecheck && pnpm --dir ui test
```

Browser tests (`pnpm --dir ui test:e2e`) need the release binary; the README
lists the steps. Write the test first where you can. After you change an admin
route, regenerate `openapi/admin.json` (the command is in the README); CI
checks that it is current.

## Conventions

[`docs/CONVENTIONS.md`](docs/CONVENTIONS.md) is binding: read it before you
change code. Never put real keys, passwords or tokens in code, tests or
fixtures; CI scans for secrets.

## Commits and pull requests

- One logical change per commit, with a short imperative subject and a
  conventional prefix such as `feat:`, `fix:`, `docs:`, `test:` or `chore:`.
- Stage specific files, and do not commit `target/`, `ui/node_modules` or
  `ui/dist`.
- Explain in the pull request what changed and why, and how you tested it.

## Proposing a feature

Open an issue with the "Feature request" template first and describe the
problem you have, not only the solution. Phase 2 plans (guardrails, single
sign-on and others) are listed in the design document; a discussion before code
saves work for both of us.

## Bugs and security

Use the "Bug report" template. For a vulnerability, do not open an issue: see
[`SECURITY.md`](SECURITY.md).

## Conduct

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).
