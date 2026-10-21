# ufaigw: the technical name of Ultrafast

## Intent

The project owns `ufaigw.com` (the MCP catalog is already served from
`mcp.ufaigw.com`). Every name a person types, installs or pulls becomes
`ufaigw`, the way Kubernetes is typed `kubectl`. "Ultrafast" stays the
product's name where a person reads it as a name: prose, the console, the
README title, release notes.

Why now: nothing has been published to npm, PyPI or crates.io yet (the
publish workflow has only done dry runs), and `ufaigw` is free on all three.
The only things out under the old name are the `ultrafast` binaries of the
2.1.0 and 2.1.1 GitHub releases and the image
`ghcr.io/techgopal/ultrafast-ai-gateway`. After the first package publish a
rename costs far more.

## Goals

- The command is `ufaigw` (`ufaigw serve`, `ufaigw --version`).
- Release archives, the image, and every client package carry `ufaigw`.
- One release makes the whole change, with upgrade notes that say exactly
  what a 2.1.x user changes.

## Non-goals

- No `ultrafast` alias, shim or compatibility binary (the owner's decision).
- The GitHub repository keeps its name `techgopal/ultrafast-ai-gateway`:
  renaming it breaks links and the trusted-publisher settings to come.
- The `UF_*` settings keep their names; they already match.
- The production instance on the maintainer's machine (systemd user service
  `ultrafast-gateway`) is not touched. The owner redeploys it when they
  choose; CONVENTIONS keeps describing it by its current names until then.

## Anti-goals

- An upgrade that silently starts empty. A 2.1.x Docker user who keeps
  `-v ultrafast-data:/var/lib/ultrafast` must not get a fresh gateway that
  looks like their data is gone.
- Old exports that no longer import. A config file exported by 2.1.x must
  still import.
- "Ultrafast" disappearing from places a person reads it as the product's
  name.

## The names

| Thing | Before | After |
| --- | --- | --- |
| Command (`[[bin]]`, clap `name`) | `ultrafast` | `ufaigw` |
| Gateway crate (not published) | `ultrafast-gateway` (`ultrafast_gateway`) | `ufaigw-gateway` (`ufaigw_gateway`) |
| Published crates | `ultrafast-translate`, `ultrafast-client` | `ufaigw-translate`, `ufaigw-client` |
| Other crates | `ultrafast-client-wasm`, `ultrafast-client-py` | `ufaigw-client-wasm`, `ufaigw-client-py` |
| Release archives | `ultrafast-vX.Y.Z-<target>.tar.gz` / `.zip` | `ufaigw-vX.Y.Z-<target>.tar.gz` / `.zip` |
| Image | `ghcr.io/techgopal/ultrafast-ai-gateway` | `ghcr.io/techgopal/ufaigw` |
| Image user, home, data | `ultrafast`, `/var/lib/ultrafast` | `ufaigw`, `/var/lib/ufaigw` |
| npm | `@ultrafast/client`, `@ultrafast/admin` | `@ufaigw/client`, `@ufaigw/admin` |
| PyPI | `ultrafast`, `ultrafast-admin` | `ufaigw`, `ufaigw-admin` |
| Python import | `import ultrafast`, `ultrafast_admin`, `ultrafast._native` | `import ufaigw`, `ufaigw_admin`, `ufaigw._native` |
| Console package | `ultrafast-console` | `ufaigw-console` |
| User-Agent | `ultrafast/X`, `ultrafast-gateway/X` | `ufaigw/X` |
| OTel service name default | `ultrafast` | `ufaigw` |
| Download file names | `ultrafast-<stamp>.db`, `ultrafast-config-<stamp>.json` | `ufaigw-<stamp>.db`, `ufaigw-config-<stamp>.json` |
| Alert payload `gateway` | `ultrafast X` | `ufaigw X` |
| Tracing targets | `ultrafast::…` | `ufaigw::…` |

Stays "Ultrafast": the console brand (`Brand.tsx`, sidebar label, page
title), the README title and prose, release-note titles ("Ultrafast 2.2.0"),
`docs/brand` artwork, clap's `about` text. Type names that spell the product
(`UltrafastError` in the Python and TypeScript clients) stay: they read as
the product's name inside the `ufaigw` package.

## Stored formats stay as they are

Names inside data that already exists outside this repository are formats,
not branding, and do not change:

- The config export's `"format": "ultrafast-config"`. Import keeps accepting
  it; export keeps writing it.
- The MCP `_meta` key `"ultrafast"` and the route marker `"ultrafast-route"`.
- Database contents and migrations.

## The Docker upgrade guard

The image's data directory moves to `/var/lib/ufaigw`. When the gateway
starts with `UF_DATA_DIR=/var/lib/ufaigw` and no database there, but
`/var/lib/ultrafast/gateway.db` exists, it refuses to start and says, in one
line, to mount the volume at `/var/lib/ufaigw` instead. This is a startup
check, not an alias: nothing is read from or moved out of the old path. The
check is only in the image's default path, so a binary install is unaffected
(its default data directory is `./data`, unchanged).

## Version

The rename is a breaking change for anyone running 2.1.x: the command,
the image and the archive names change. The release is **3.0.0**. All
package versions move together as `docs/RELEASING.md` already requires.

## Upgrade notes (in the 3.0.0 release notes and CHANGELOG)

- Binary: the archive and the command are `ufaigw`. Replace `ultrafast` in
  scripts and service files (`ExecStart`).
- Docker: `ghcr.io/techgopal/ufaigw:3.0.0`; mount the existing volume at
  `/var/lib/ufaigw` (`-v ultrafast-data:/var/lib/ufaigw`). The data is the
  same; only the mount point changes. The volume's files are owned by the
  old image's `ultrafast` user: the new image's `ufaigw` user must get the
  same uid (pin it in the Dockerfile to the uid the old image used) so no
  `chown` is needed.
- OTel: the default service name is now `ufaigw`; set
  `UF_OTEL_SERVICE_NAME=ultrafast` to keep existing dashboards.
- No database migration.

## Owner steps (outside the repository)

- Create the npm scope `@ufaigw`.
- Register PyPI pending publishers for `ufaigw` and `ufaigw-admin`, and use
  `ufaigw-translate` and `ufaigw-client` for crates.io, per
  `docs/RELEASING.md` (which is updated to the new names).

## What is left to the builder

- The order of the mechanical renames and how they are batched, as long as
  every commit passes the gates in `docs/CONVENTIONS.md`.
- Test names and fixtures that mention the old name, where they test
  something other than the stored formats above.
- `docs/brand` alt text and the banner: alt text follows the table above;
  the artwork is not redrawn.
- Historical release notes and CHANGELOG entries for 2.1.1 and earlier are
  history and are not rewritten.
