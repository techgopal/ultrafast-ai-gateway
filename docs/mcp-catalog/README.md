# The MCP catalog

A short, curated list of MCP servers that the gateway's console offers when an
admin clicks **Add from catalog**. It is a list we publish, not a marketplace:
no ratings, no submissions form, no per-entry versions. An entry never
registers itself. It only pre-fills the URL (or command and arguments) and
names the secrets the admin has to type.

| File | What it is |
| --- | --- |
| `v1.json` | The catalog. |
| `v1.json.sig` | Base64 Ed25519 signature of the exact bytes of `v1.json`. |
| `public-key.txt` | Base64 public key. It is compiled into the gateway; gateways check every copy against it. |

The gateway fetches `https://ultrafast-ai.dev/mcp-catalog/v1.json` (and the
`.sig` next to it) every 24 hours or as the response's `max-age` says (never
more often than hourly), keeps the last copy that verified in its database,
and falls back to the copy compiled into the binary. A copy whose signature
does not match is thrown away and the console says so. Set
`UF_MCP_CATALOG=off` and the gateway makes no request for the catalog at all
(air-gapped installs); the compiled-in copy is shown.

Settings: `UF_MCP_CATALOG=off`, `UF_MCP_CATALOG_URL`, `UF_MCP_CATALOG_REFRESH`
(seconds, at least 60, default 86400).

## Entry schema

```json
{
  "id": "github",                      // lowercase letters, digits, "-"; the default server name
  "name": "GitHub",
  "description": "What it does.",
  "homepage": "https://...",
  "transport": "http",                 // "http" (url) or "stdio" (command + args)
  "url": "https://.../mcp",            // http only
  "command": "npx",                    // stdio only: no shell, argv exactly as listed
  "args": ["-y", "some-mcp-server"],
  "env": [{ "name": "TOKEN", "secret": true, "help": "Where to get it." }],
  "auth": "bearer",                    // none | bearer | header | oauth
  "auth_header": "x-api-key",          // only for auth "header"
  "tags": ["code"]
}
```

For an `http` entry with `bearer` or `header` auth, exactly one `env` item has
`secret: true`: the value the admin types is the credential the gateway sends.
For a `stdio` entry the `env` items are the environment variables the child
process receives (and the only ones it receives besides a minimal `PATH`);
stdio entries need `UF_MCP_LOCAL=on` on the gateway.

## Propose an entry

Open a pull request that adds an entry to `v1.json`. Keep it to servers that
are maintained by their vendor or widely used, that need no more than a token
or two, and whose command is a published package (no shell, no scripts). A
maintainer signs the file when releasing; you do not sign it yourself.

## Signing

The release workflow (`.github/workflows/release.yml`, job `catalog`) signs
`v1.json` with the key in the repository secret `MCP_CATALOG_SIGNING_KEY`
(base64 of the 32-byte Ed25519 seed), checks the signature against
`public-key.txt` and publishes the three files on GitHub Pages under
`/mcp-catalog/`. `pages.yml` publishes the committed files when they change on
`main`.

```sh
cargo run -p mcp-catalog-sign -- keygen                      # new key pair; keep the private line secret
MCP_CATALOG_SIGNING_KEY=... cargo run -p mcp-catalog-sign -- sign docs/mcp-catalog/v1.json
cargo run -p mcp-catalog-sign -- verify docs/mcp-catalog/v1.json docs/mcp-catalog/public-key.txt
```

A gateway test fails when `v1.json` is edited without signing it again.
Rotating the key means changing `public-key.txt` and releasing a new binary.
