# ultrafast (Python)

Python client for an Ultrafast gateway, or for an LLM provider directly. It
wraps the Rust client (`crates/client`) with PyO3: one wheel per platform for
Python 3.9 and newer (abi3). It does not retry, route, cache or break circuits.

```sh
pip install --pre ultrafast
```

The wheels appear on PyPI from v2.0.0-beta.4.

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

## Tools and images

Messages are `(role, content)` tuples, `Message` objects, or OpenAI-shaped
dicts (`content` a string or a list of `text` / `image_url` parts, an assistant's
`tool_calls`, a tool's `tool_call_id`):

```python
messages = [{"role": "user", "content": [
    {"type": "text", "text": "What is in this picture?"},
    {"type": "image_url", "image_url": {"url": "data:image/png;base64,..."}},
]}]
tools = [{"name": "weather", "description": "Current weather", "parameters": {"type": "object"}}]
reply = client.chat(
    "gpt-4o",
    messages,
    tools=tools,
    tool_choice="auto",           # "auto", "none", "required", or a tool's name
    parallel_tool_calls=True,
)
for call in reply.tool_calls:     # ToolCall(id, name, arguments)  (arguments is JSON text)
    ...
```

`tools` entries are flat `{name, description?, parameters?, strict?}` dicts (OpenAI's
`{"type": "function", "function": {...}}` is accepted too); `strict` is sent to
OpenAI and Azure only. Send results back by
appending the assistant turn and one tool message per call, then calling again:

```python
messages.append({"role": "assistant", "content": reply.content, "tool_calls": reply.tool_calls})
for call in reply.tool_calls:
    messages.append({"role": "tool", "tool_call_id": call.id, "content": run_tool(call)})
reply = client.chat("gpt-4o", messages, tools=tools)
```

`tool_calls` may hold `ToolCall` objects, flat `{id, name, arguments}` dicts or
OpenAI-shaped dicts; `Message(role, content, tool_calls=..., tool_call_id=...)` works too.
`chat_stream` yields `Delta`, `ToolCallStart(index, id, name)`,
`ToolCallDelta(index, arguments)` and `Done`. Messages are checked by the same
parser the gateway uses; a bad one raises `InvalidRequestError` before anything
is sent. Direct provider targets accept the same arguments.

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

`tests/test_fixtures.py` runs the parity fixtures in `clients/fixtures/*.json`,
the same files the Rust and TypeScript clients run.

No Python headers or sudo are needed on Linux. Publishing wheels needs the
owner's credentials; `.github/workflows/clients.yml` only builds them.

The blocking client releases the GIL while it waits and checks for signals
every 100 ms, so Ctrl-C interrupts a call that is waiting on the network.

Do not share a client across `os.fork()`: the child would inherit the
runtime's threads in a state they cannot be used in. Create the client after
the fork (for example in each worker process).
