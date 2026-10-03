"""Python client for the Ultrafast gateway, or for a provider directly.

    import ultrafast
    client = ultrafast.Client(ultrafast.gateway("http://localhost:3000", key))
    reply = client.chat("gpt-4o", [{"role": "user", "content": "hi"}])

The client does not retry, route, cache or break circuits: every error says
whether trying again could help (`retryable`) and how long to wait (`retry_after`).
"""

from __future__ import annotations

from typing import Any, Dict, Iterable, List, Optional, Sequence, Union

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
    "UpstreamError",
    "Usage",
    "anthropic",
    "azure",
    "gateway",
    "gemini",
    "openai",
    "openai_compatible",
]

_ROLES = ("system", "user", "assistant")


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


def _messages(messages: Iterable[Any]) -> List[tuple]:
    out = []
    for m in messages:
        if isinstance(m, Message):
            role, content = m.role, m.content
        elif isinstance(m, dict):
            role, content = m.get("role"), m.get("content")
        else:
            raise TypeError("a message is a dict with role and content, or a Message")
        if not isinstance(role, str) or role not in _ROLES:
            raise ValueError(f"a message role is one of {', '.join(_ROLES)}")
        if not isinstance(content, str):
            raise TypeError("a message content is a string (text only)")
        out.append((role, content))
    return out


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


def _chat_args(model, messages, max_tokens, temperature, top_p, stop, tags):
    if not isinstance(model, str):
        raise TypeError("model is a string")
    return (
        model,
        _messages(messages),
        max_tokens,
        None if temperature is None else float(temperature),
        None if top_p is None else float(top_p),
        _stop(stop),
        _tags(tags),
    )


def _embed_args(model, input, dimensions, tags):
    if not isinstance(model, str):
        raise TypeError("model is a string")
    inputs = [input] if isinstance(input, str) else list(input)
    if not all(isinstance(i, str) for i in inputs):
        raise TypeError("input is a string or a list of strings")
    return model, inputs, dimensions, _tags(tags)


def _usage(u) -> Optional[Usage]:
    return None if u is None else Usage(*u)


def _chat(t) -> ChatResponse:
    id, model, content, finish, usage = t
    return ChatResponse(id, model, content, finish, _usage(usage))


def _event(t) -> StreamEvent:
    kind, text, finish, usage = t
    if kind == "delta":
        return Delta(text)
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
        messages: Sequence[Union[Message, Dict[str, str]]],
        *,
        max_tokens: Optional[int] = None,
        temperature: Optional[float] = None,
        top_p: Optional[float] = None,
        stop: Union[None, str, Sequence[str]] = None,
        tags: Optional[Dict[str, str]] = None,
    ) -> ChatResponse:
        args = _chat_args(model, messages, max_tokens, temperature, top_p, stop, tags)
        return _chat(self._native.chat(*args))

    def chat_stream(
        self,
        model: str,
        messages: Sequence[Union[Message, Dict[str, str]]],
        *,
        max_tokens: Optional[int] = None,
        temperature: Optional[float] = None,
        top_p: Optional[float] = None,
        stop: Union[None, str, Sequence[str]] = None,
        tags: Optional[Dict[str, str]] = None,
    ) -> ChatStream:
        """Sends the request and returns once the answer starts; a refusal raises here."""
        args = _chat_args(model, messages, max_tokens, temperature, top_p, stop, tags)
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

    async def _ensure(self):
        if self._native is None:
            self._native = await self._open()

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
        messages: Sequence[Union[Message, Dict[str, str]]],
        *,
        max_tokens: Optional[int] = None,
        temperature: Optional[float] = None,
        top_p: Optional[float] = None,
        stop: Union[None, str, Sequence[str]] = None,
        tags: Optional[Dict[str, str]] = None,
    ) -> ChatResponse:
        args = _chat_args(model, messages, max_tokens, temperature, top_p, stop, tags)
        return _chat(await self._native.chat(*args))

    def chat_stream(
        self,
        model: str,
        messages: Sequence[Union[Message, Dict[str, str]]],
        *,
        max_tokens: Optional[int] = None,
        temperature: Optional[float] = None,
        top_p: Optional[float] = None,
        stop: Union[None, str, Sequence[str]] = None,
        tags: Optional[Dict[str, str]] = None,
    ) -> AsyncChatStream:
        args = _chat_args(model, messages, max_tokens, temperature, top_p, stop, tags)
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
