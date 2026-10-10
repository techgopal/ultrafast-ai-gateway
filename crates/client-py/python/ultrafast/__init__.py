"""Python client for the Ultrafast gateway, or for a provider directly.

    import ultrafast
    client = ultrafast.Client(ultrafast.gateway("http://localhost:3000", key))
    reply = client.chat("gpt-4o", [{"role": "user", "content": "hi"}])

The client does not retry, route, cache or break circuits: every error says
whether trying again could help (`retryable`) and how long to wait (`retry_after`).
"""

from __future__ import annotations

from typing import Any, Dict, Iterable, List, Optional, Sequence, Tuple, Union

import asyncio
import json

from . import _native
from ._errors import (
    AuthenticationError,
    Error,
    InvalidRequestError,
    MalformedError,
    NetworkError,
    NotFoundError,
    PermissionDeniedError,
    RateLimitError,
    RequestTimeoutError,
    UpstreamError,
)
from ._types import (
    ChatResponse,
    Delta,
    Done,
    EmbeddingsResponse,
    Message,
    StreamEvent,
    ToolCall,
    ToolCallDelta,
    ToolCallStart,
    Usage,
)

Target = _native.Target

__all__ = [
    "AsyncClient",
    "AuthenticationError",
    "ChatResponse",
    "Client",
    "Delta",
    "Done",
    "EmbeddingsResponse",
    "Error",
    "InvalidRequestError",
    "MalformedError",
    "Message",
    "NetworkError",
    "NotFoundError",
    "PermissionDeniedError",
    "RateLimitError",
    "RequestTimeoutError",
    "StreamEvent",
    "Target",
    "ToolCall",
    "ToolCallDelta",
    "ToolCallStart",
    "UpstreamError",
    "Usage",
    "anthropic",
    "azure",
    "gateway",
    "gemini",
    "openai",
    "openai_compatible",
]

_ROLES = ("system", "user", "assistant", "tool")


def gateway(base_url: str, key: str) -> Target:
    """An Ultrafast gateway; a trailing `/v1` on `base_url` is accepted and ignored."""
    return _native.gateway(base_url, key)


def openai(key: str, base_url: Optional[str] = None) -> Target:
    """OpenAI directly; `base_url` includes the version segment (default https://api.openai.com/v1)."""
    return _native.openai(key, base_url)


def anthropic(key: str, base_url: Optional[str] = None) -> Target:
    return _native.anthropic(key, base_url)


def gemini(key: str, base_url: Optional[str] = None) -> Target:
    return _native.gemini(key, base_url)


def azure(endpoint: str, key: str, api_version: Optional[str] = None) -> Target:
    """Azure OpenAI; the call's model is the deployment name."""
    return _native.azure(endpoint, key, api_version)


def openai_compatible(base_url: str, key: str) -> Target:
    """Any API speaking OpenAI's format (Groq, Mistral, OpenRouter, Ollama); `base_url` includes `/v1`."""
    return _native.openai_compatible(base_url, key)


def _messages(messages: Iterable[Any]) -> str:
    """The messages as the JSON text of an OpenAI-shaped array, which the
    native layer parses with the gateway's own parser."""
    out = []
    for m in messages:
        if isinstance(m, Message):
            d: Dict[str, Any] = {"role": m.role, "content": m.content}
            if m.tool_calls is not None:
                d["tool_calls"] = m.tool_calls
            if m.tool_call_id is not None:
                d["tool_call_id"] = m.tool_call_id
            out.append(_check_message(d))
        elif isinstance(m, dict):
            out.append(_check_message(m))
        elif isinstance(m, tuple) and len(m) == 2:
            out.append(_check_message({"role": m[0], "content": m[1]}))
        else:
            raise TypeError("a message is a dict, a (role, content) tuple, or a Message")
    return json.dumps(out)


def _check_message(m: Dict[str, Any]) -> Dict[str, Any]:
    role, content = m.get("role"), m.get("content")
    if not isinstance(role, str) or role not in _ROLES:
        raise ValueError(f"a message role is one of {', '.join(_ROLES)}")
    if content is not None and not isinstance(content, (str, list)):
        raise TypeError("a message content is a string, a list of parts, or None")
    calls = m.get("tool_calls")
    if calls is None:
        return m
    if isinstance(calls, (str, bytes, dict)):
        raise TypeError("tool_calls is a list of ToolCall or dicts")
    return {**m, "tool_calls": [_tool_call(c) for c in calls]}


def _tool_call(c: Any) -> Dict[str, Any]:
    """A `ToolCall`, a flat {id, name, arguments} dict or an OpenAI-shaped dict, as OpenAI's."""
    if isinstance(c, ToolCall):
        c = {"id": c.id, "name": c.name, "arguments": c.arguments}
    if not isinstance(c, dict):
        raise TypeError("tool_calls is a list of ToolCall or dicts")
    if "function" in c:
        return c
    return {
        "id": c.get("id"),
        "type": "function",
        "function": {"name": c.get("name"), "arguments": c.get("arguments")},
    }


def _tools(tools: Optional[Sequence[Dict[str, Any]]]) -> Optional[str]:
    if tools is None:
        return None
    if isinstance(tools, (str, bytes, dict)) or not all(isinstance(t, dict) for t in tools):
        raise TypeError("tools is a list of dicts")
    return json.dumps([_tool(t) for t in tools])


def _tool(t: Dict[str, Any]) -> Dict[str, Any]:
    """Flat {name, description?, parameters?} or OpenAI's {type:"function", function:{...}}."""
    if "function" in t or "type" in t:
        return t
    return {"type": "function", "function": t}


def _tool_choice(choice: Optional[str]) -> Optional[str]:
    if choice is not None and not isinstance(choice, str):
        raise TypeError('tool_choice is "auto", "none", "required" or a tool name')
    return choice


def _parallel(v: Optional[bool]) -> Optional[bool]:
    if v is not None and not isinstance(v, bool):
        raise TypeError("parallel_tool_calls is a bool")
    return v


def _response_format(f: Optional[Dict[str, Any]]) -> Optional[str]:
    """OpenAI's `response_format`: {"type": "text" | "json_object"} or
    {"type": "json_schema", "json_schema": {"name", "schema", "strict"?, "description"?}}."""
    if f is None:
        return None
    if not isinstance(f, dict):
        raise TypeError("response_format is a dict")
    return json.dumps(f)


def _stop(stop: Union[None, str, Sequence[str]]) -> Optional[List[str]]:
    if stop is None:
        return None
    if isinstance(stop, str):
        return [stop]
    stop = list(stop)
    if not all(isinstance(s, str) for s in stop):
        raise TypeError("stop is a string or a list of strings")
    return stop


def _tags(tags: Optional[Dict[str, str]]) -> Optional[Dict[str, str]]:
    if tags is None:
        return None
    if not isinstance(tags, dict) or not all(
        isinstance(k, str) and isinstance(v, str) for k, v in tags.items()
    ):
        raise TypeError("tags is a dict of strings to strings")
    return dict(tags)


_U32_MAX = 2**32 - 1


def _u32(name: str, value: Optional[int]) -> Optional[int]:
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{name} is an int")
    if not 0 <= value <= _U32_MAX:
        raise ValueError(f"{name} is between 0 and {_U32_MAX}")
    return value


def _chat_args(
    model, messages, max_tokens, temperature, top_p, stop, tags, tools, tool_choice, parallel_tool_calls, response_format
):
    if not isinstance(model, str):
        raise TypeError("model is a string")
    return (
        model,
        _messages(messages),
        _u32("max_tokens", max_tokens),
        None if temperature is None else float(temperature),
        None if top_p is None else float(top_p),
        _stop(stop),
        _tags(tags),
        _tools(tools),
        _tool_choice(tool_choice),
        _parallel(parallel_tool_calls),
        _response_format(response_format),
    )


def _embed_args(model, input, dimensions, tags):
    if not isinstance(model, str):
        raise TypeError("model is a string")
    inputs = [input] if isinstance(input, str) else list(input)
    if not all(isinstance(i, str) for i in inputs):
        raise TypeError("input is a string or a list of strings")
    return model, inputs, _u32("dimensions", dimensions), _tags(tags)


def _usage(u) -> Optional[Usage]:
    return None if u is None else Usage(*u)


def _chat(t) -> ChatResponse:
    id, model, content, finish, usage, calls = t
    return ChatResponse(id, model, content, finish, _usage(usage), [ToolCall(*c) for c in calls])


def _event(t) -> StreamEvent:
    kind, text, finish, usage, index, id, name = t
    if kind == "delta":
        return Delta(text)
    if kind == "tool_call_start":
        return ToolCallStart(index, id, name)
    if kind == "tool_call_delta":
        return ToolCallDelta(index, text)
    return Done(finish, _usage(usage))


def _embeddings(t) -> EmbeddingsResponse:
    return EmbeddingsResponse(*t)


def _timeout(timeout: Optional[float]) -> Optional[float]:
    if timeout is None:
        return None
    if isinstance(timeout, bool) or not isinstance(timeout, (int, float)) or timeout <= 0:
        raise ValueError("timeout is a positive number of seconds")
    return float(timeout)


class ChatStream:
    """Events of a streaming answer, in order. A stream that fails or is cut
    short raises after the events that did arrive; nothing is dropped silently.
    """

    def __init__(self, native):
        self._native = native

    def __iter__(self):
        return self

    def __next__(self) -> StreamEvent:
        t = self._native.next()
        if t is None:
            raise StopIteration
        return _event(t)

    def close(self) -> None:
        self._native.close()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


class Client:
    """Blocking client. The GIL is released while a call waits on the network,
    so other threads run; the client is safe to share between threads.

    `timeout` (seconds, default 120) covers a whole `chat`/`embed`, and for a
    stream the wait for the answer and the longest silence between chunks.
    `max_response_bytes` (default 32 MiB) caps an answer or an error body.
    """

    def __init__(
        self,
        target: Target,
        *,
        timeout: Optional[float] = None,
        max_response_bytes: Optional[int] = None,
    ):
        self._native = _native.Client(target, _timeout(timeout), max_response_bytes)

    def __repr__(self) -> str:
        return f"Client({self._native.target_repr()})"

    def chat(
        self,
        model: str,
        messages: Sequence[Union[Message, Dict[str, Any], Tuple[str, str]]],
        *,
        max_tokens: Optional[int] = None,
        temperature: Optional[float] = None,
        top_p: Optional[float] = None,
        stop: Union[None, str, Sequence[str]] = None,
        tags: Optional[Dict[str, str]] = None,
        tools: Optional[Sequence[Dict[str, Any]]] = None,
        tool_choice: Optional[str] = None,
        parallel_tool_calls: Optional[bool] = None,
        response_format: Optional[Dict[str, Any]] = None,
    ) -> ChatResponse:
        args = _chat_args(
            model, messages, max_tokens, temperature, top_p, stop, tags, tools, tool_choice, parallel_tool_calls, response_format
        )
        return _chat(self._native.chat(*args))

    def chat_stream(
        self,
        model: str,
        messages: Sequence[Union[Message, Dict[str, Any], Tuple[str, str]]],
        *,
        max_tokens: Optional[int] = None,
        temperature: Optional[float] = None,
        top_p: Optional[float] = None,
        stop: Union[None, str, Sequence[str]] = None,
        tags: Optional[Dict[str, str]] = None,
        tools: Optional[Sequence[Dict[str, Any]]] = None,
        tool_choice: Optional[str] = None,
        parallel_tool_calls: Optional[bool] = None,
        response_format: Optional[Dict[str, Any]] = None,
    ) -> ChatStream:
        """Sends the request and returns once the answer starts; a refusal raises here."""
        args = _chat_args(
            model, messages, max_tokens, temperature, top_p, stop, tags, tools, tool_choice, parallel_tool_calls, response_format
        )
        return ChatStream(self._native.chat_stream(*args))

    def embed(
        self,
        model: str,
        input: Union[str, Sequence[str]],
        *,
        dimensions: Optional[int] = None,
        tags: Optional[Dict[str, str]] = None,
    ) -> EmbeddingsResponse:
        return _embeddings(self._native.embed(*_embed_args(model, input, dimensions, tags)))


class AsyncChatStream:
    """Async twin of `ChatStream`. Use `async for e in client.chat_stream(...)`,
    or `stream = await client.chat_stream(...)` to see a refusal before iterating.
    """

    def __init__(self, open_native):
        self._open = open_native
        self._native = None
        self._opening = None

    async def _ensure(self):
        if self._native is not None:
            return
        # One opening, however many callers arrive first; shielded so one
        # caller being cancelled does not cancel the others' opening.
        if self._opening is None:
            self._opening = asyncio.ensure_future(self._open())
        self._native = await asyncio.shield(self._opening)

    def __await__(self):
        async def opened():
            await self._ensure()
            return self

        return opened().__await__()

    def __aiter__(self):
        return self

    async def __anext__(self) -> StreamEvent:
        await self._ensure()
        t = await self._native.next()
        if t is None:
            raise StopAsyncIteration
        return _event(t)

    async def aclose(self) -> None:
        if self._native is not None:
            self._native.close()

    async def __aenter__(self):
        await self._ensure()
        return self

    async def __aexit__(self, *exc):
        await self.aclose()


class AsyncClient:
    """asyncio client; same arguments and results as `Client`."""

    def __init__(
        self,
        target: Target,
        *,
        timeout: Optional[float] = None,
        max_response_bytes: Optional[int] = None,
    ):
        self._native = _native.AsyncClient(target, _timeout(timeout), max_response_bytes)

    def __repr__(self) -> str:
        return f"AsyncClient({self._native.target_repr()})"

    async def chat(
        self,
        model: str,
        messages: Sequence[Union[Message, Dict[str, Any], Tuple[str, str]]],
        *,
        max_tokens: Optional[int] = None,
        temperature: Optional[float] = None,
        top_p: Optional[float] = None,
        stop: Union[None, str, Sequence[str]] = None,
        tags: Optional[Dict[str, str]] = None,
        tools: Optional[Sequence[Dict[str, Any]]] = None,
        tool_choice: Optional[str] = None,
        parallel_tool_calls: Optional[bool] = None,
        response_format: Optional[Dict[str, Any]] = None,
    ) -> ChatResponse:
        args = _chat_args(
            model, messages, max_tokens, temperature, top_p, stop, tags, tools, tool_choice, parallel_tool_calls, response_format
        )
        return _chat(await self._native.chat(*args))

    def chat_stream(
        self,
        model: str,
        messages: Sequence[Union[Message, Dict[str, Any], Tuple[str, str]]],
        *,
        max_tokens: Optional[int] = None,
        temperature: Optional[float] = None,
        top_p: Optional[float] = None,
        stop: Union[None, str, Sequence[str]] = None,
        tags: Optional[Dict[str, str]] = None,
        tools: Optional[Sequence[Dict[str, Any]]] = None,
        tool_choice: Optional[str] = None,
        parallel_tool_calls: Optional[bool] = None,
        response_format: Optional[Dict[str, Any]] = None,
    ) -> AsyncChatStream:
        args = _chat_args(
            model, messages, max_tokens, temperature, top_p, stop, tags, tools, tool_choice, parallel_tool_calls, response_format
        )
        return AsyncChatStream(lambda: self._native.chat_stream(*args))

    async def embed(
        self,
        model: str,
        input: Union[str, Sequence[str]],
        *,
        dimensions: Optional[int] = None,
        tags: Optional[Dict[str, str]] = None,
    ) -> EmbeddingsResponse:
        return _embeddings(await self._native.embed(*_embed_args(model, input, dimensions, tags)))
