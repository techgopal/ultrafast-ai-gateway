# ultrafast (Python)

Python client for an Ultrafast gateway, or for an LLM provider directly. It
wraps the Rust client (`crates/client`) with PyO3: one wheel per platform for
Python 3.9 and newer (abi3). It does not retry, route, cache or break circuits.

```python
import ultrafast

client = ultrafast.Client(ultrafast.gateway("http://localhost:3000", "uf-...key"))
reply = client.chat("gpt-4o", [{"role": "user", "content": "Say hi."}], tags={"team": "search"})
print(reply.content, reply.usage)

for event in client.chat_stream("gpt-4o", [{"role": "user", "content": "Count."}]):
    if isinstance(event, ultrafast.Delta):
        print(event.text, end="")

vectors = client.embed("text-embedding-3-small", ["a", "b"]).vectors
```

Async, same arguments:

```python
client = ultrafast.AsyncClient(ultrafast.openai(key))
reply = await client.chat("gpt-4o", messages)
async for event in client.chat_stream("gpt-4o", messages):
    ...
```

## Targets

`gateway(base_url, key)` (a trailing `/v1` is ignored), `openai(key, base_url=None)`,
`anthropic(key, base_url=None)`, `gemini(key, base_url=None)`,
`azure(endpoint, key, api_version=None)` (the model is the deployment),
`openai_compatible(base_url, key)`. `Client(target, timeout=None, max_response_bytes=None)`;
the timeout is in seconds (default 120).

## Errors

Every failure is an `ultrafast.Error` with `.kind`, `.status`, `.retryable` and
`.retry_after` (seconds, when the server sent one). Subclasses: `AuthenticationError`,
`PermissionDeniedError`, `NotFoundError`, `InvalidRequestError`, `RateLimitError`,
`UpstreamError`, `NetworkError`, `RequestTimeoutError`, `MalformedError`.
A stream that fails or is cut short yields the text that arrived, then raises.
Messages, errors and `repr` never contain the key.

## Tags

`tags={"team": "search"}` is sent as the `x-uf-tags` header to a gateway target
only, never to a provider (at most 1 KiB of JSON).

## Build and test

```
python3 -m venv .venv && . .venv/bin/activate
pip install maturin pytest pytest-asyncio
maturin develop
pytest
```

No Python headers or sudo are needed on Linux. Publishing wheels needs the
owner's credentials; `.github/workflows/clients.yml` only builds them.

The blocking client releases the GIL while it waits; a Ctrl-C is seen after
the call returns or times out.
