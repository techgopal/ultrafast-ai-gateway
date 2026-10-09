# ultrafast-admin

Typed Python client for the UltraFast AI Gateway admin API (`/api`): providers,
models, keys, budgets, limits, teams, users, alerts, guardrails, routes, tokens,
settings, usage, logs, audit, backup and configuration. Synchronous and
asynchronous calls, generated from `openapi/admin.json`.

```sh
pip install ultrafast-admin   # Python 3.11 or newer
```

Create an access token in the console (Account, Access tokens), then:

```python
from ultrafast_admin import AdminClient, AdminApiError
from ultrafast_admin._generated.api.providers import providers_list, providers_create
from ultrafast_admin._generated.models import CreateProviderRequest

api = AdminClient("https://gateway.example.com", token)  # timeout=30.0 seconds

providers = api.call(providers_list.sync_detailed(client=api.client)).providers

try:
    api.call(
        providers_create.sync_detailed(
            client=api.client,
            body=CreateProviderRequest(name="openai", kind="openai", base_url="https://api.openai.com/v1", api_key=key),
        )
    )
except AdminApiError as error:
    print(error.status, error.code, error.message, error.fields)
```

Calls live in `ultrafast_admin._generated.api.<tag>.<operation_id>`; each module
has `sync`, `sync_detailed`, `asyncio` and `asyncio_detailed`. The async form:

```python
async with AdminClient(url, token) as api:
    response = await providers_list.asyncio_detailed(client=api.client)
    providers = api.call(response).providers
```

`api.client` is the generated `AuthenticatedClient`. Whichever function you use,
a call that fails raises `AdminApiError(status, code, message, fields)`:

| status | code | when |
| --- | --- | --- |
| 401 | `unauthenticated` | the token is unknown or revoked |
| 403 | `forbidden` | the token's user may not do this |
| 404 | `not_found` | |
| 409 | e.g. `provider_exists` | |
| 422 | `validation_failed` | `fields` names each invalid field |
| other | `http_<status>` | an answer that is not the gateway's error shape |
| 0 | `timeout`, `network_error` | no answer |

The token is sent only as a bearer header and is never printed: it does not
appear in an error, a `repr`, a `str` or a message, and an `AdminClient` cannot
be pickled. It is readable by code you run (`api.client.token`, the httpx
client's headers), as with any HTTP client.

`timeout` (seconds, default 30) is a total for one call: connecting, sending and
receiving the whole answer. A call that runs out, or whose answer stalls or
drips, raises `AdminApiError` with status 0 and code `timeout`. Use after
`close()` raises `AdminApiError` (status 0, `network_error`).
`api.client.with_timeout(httpx.Timeout(5))`, `.with_headers(...)` and
`.with_cookies(...)` return copies that keep the bearer header and the same
error handling.

Downloads: `api.download_backup()` returns the bytes of a SQLite file,
`api.export_config()` the configuration file as a `dict` (async:
`adownload_backup`, `aexport_config`); each takes `timeout=` seconds for that
call.

## Regenerating

`./gen.sh` rewrites `ultrafast_admin/_generated/` from `../../openapi/admin.json`
with a pinned openapi-python-client in a virtualenv under `~/.cache`. Run it twice
and nothing changes. Do not edit generated files.

## Tests

`pip install -e '.[test]' && pytest` starts the real gateway (a debug build,
`cargo build -p ultrafast-gateway`, or the binary named by `UF_E2E_BINARY`) on a
free port with a temporary data directory under `~/.cache`. `CARGO_TARGET_DIR` is
honoured (the binary is found with `cargo metadata`).
