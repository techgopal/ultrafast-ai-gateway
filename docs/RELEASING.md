# Releasing

A tag `vX.Y.Z[-pre]` that names the version in `Cargo.toml` runs two workflows:
`release.yml` (binaries, a draft GitHub release, the image) and `publish.yml`
(the client packages). All package versions move together: `Cargo.toml`
(`[workspace.package]` and the `ultrafast-translate` dependency versions in
`crates/client` and `crates/client-wasm`), `clients/ts/package.json`,
`clients/admin-ts/package.json`, `crates/client-py/pyproject.toml` and
`clients/admin-py/pyproject.toml`. Python uses the PEP 440 spelling
(`2.0.0-beta.4` is `2.0.0b4`). `publish.yml` refuses a tag that does not match
all of them.

## What is published

| Registry | Packages |
| --- | --- |
| crates.io | `ultrafast-translate`, then `ultrafast-client` |
| PyPI | `ultrafast` (wheels and sdist from `crates/client-py`), `ultrafast-admin` |
| npm | `@ultrafast/client`, `@ultrafast/admin` (public, with provenance, tag `beta` for a pre-release) |

## One-time owner steps

Until these are done the workflow only does dry runs.

1. **crates.io.** Either create an API token (scopes `publish-new` and
   `publish-update`) and store it as the repository secret
   `CARGO_REGISTRY_TOKEN`, or, after the first publish creates the crates,
   add a trusted publisher for each crate (repository
   `techgopal/ultrafast-ai-gateway`, workflow `publish.yml`, environment
   `release`). The workflow uses the token when the secret exists and trusted
   publishing otherwise. crates.io cannot register a trusted publisher for a
   crate that does not exist yet, so the first publish needs the token.
2. **PyPI.** For each project (`ultrafast`, `ultrafast-admin`) add a trusted
   publisher (a "pending publisher" at https://pypi.org/manage/account/publishing/
   while the project does not exist): owner `techgopal`, repository
   `ultrafast-ai-gateway`, workflow `publish.yml`, environment `release`.
3. **npm.** Under each package's Settings, Trusted Publisher
   (`@ultrafast/client`, `@ultrafast/admin`): GitHub Actions, owner `techgopal`,
   repository `ultrafast-ai-gateway`, workflow `publish.yml`, environment
   `release`. The `@ultrafast` scope must exist and belong to the owner. A
   package must exist before it can have a trusted publisher: publish the first
   version by hand with `npm publish --access public --tag beta` or use a
   granular token once.

Also create the GitHub environment `release` (Settings, Environments), ideally
with the owner as a required reviewer.

Then set the repository variable `PUBLISH_LIVE=true` (Settings, Secrets and
variables, Actions, Variables). From then on a tag publishes for real.

## Dry runs

Run the workflow by hand (Actions, Publish, Run workflow) with `dry_run`
checked (the default): it builds everything, runs `cargo publish --dry-run`,
`twine check` and `pnpm publish --dry-run`, and uploads nothing. A manual run
with `dry_run` unchecked publishes only if `PUBLISH_LIVE` is `true`.

`cargo publish --dry-run -p ultrafast-client` alone fails until the same
version of `ultrafast-translate` is on crates.io. The workflow passes both
(`-p ultrafast-translate -p ultrafast-client`): cargo publishes them in
dependency order and resolves the first for the second.

Published versions cannot be replaced, only yanked or deprecated: check the
dry run of the tag's commit before setting `PUBLISH_LIVE`.
