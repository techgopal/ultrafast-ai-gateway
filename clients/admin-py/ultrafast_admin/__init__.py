"""Typed Python client for the UltraFast AI Gateway admin API (``/api``).

A call that fails raises ``AdminApiError``, whichever generated function made it.
The calls are generated from ``openapi/admin.json`` (see ``gen.sh``) and live in
``ultrafast_admin._generated.api.<tag>.<operation_id>``; each module has
``sync``, ``sync_detailed``, ``asyncio`` and ``asyncio_detailed``. This module
adds the client, the error type, and the two downloads::

    from ultrafast_admin import AdminClient
    from ultrafast_admin._generated.api.providers import providers_list

    api = AdminClient("https://gateway.example.com", token)
    providers = api.call(providers_list.sync_detailed(client=api.client)).providers
"""

from __future__ import annotations

import json
import time
from collections.abc import Callable, Mapping
from typing import Any, NoReturn, Self

import httpx

from ._generated.client import AuthenticatedClient
from ._generated.types import Response

__all__ = ["AdminApiError", "AdminClient"]

DEFAULT_TIMEOUT = 30.0
_REDACTED = "[redacted]"


class AdminApiError(Exception):
    """Every failed call of the admin API: the gateway's error body, or a failure to reach it.

    ``status`` is the HTTP status, 0 when no answer came (``network_error``,
    ``timeout``). ``code`` is the gateway's stable error name, such as
    ``not_found``; ``http_<status>`` for an answer that is not its error shape.
    ``fields`` holds a message for each field that is not valid
    (``validation_failed``); empty otherwise.
    """

    def __init__(self, status: int, code: str, message: str, fields: Mapping[str, str] | None = None) -> None:
        super().__init__(message)
        self.status = status
        self.code = code
        self.message = message
        self.fields: dict[str, str] = dict(fields or {})

    def __str__(self) -> str:
        return f"{self.code} ({self.status}): {self.message}"

    def __repr__(self) -> str:
        return f"AdminApiError(status={self.status!r}, code={self.code!r}, message={self.message!r}, fields={self.fields!r})"


def _redact(text: str, token: str) -> str:
    return text if token == "" else text.replace(token, _REDACTED)


def _error_of(status: int, content: bytes, token: str) -> AdminApiError:
    text = content.decode("utf-8", errors="replace")
    try:
        body = json.loads(text)
    except ValueError:
        body = None
    detail = body.get("error") if isinstance(body, dict) else None
    if isinstance(detail, dict):
        code, message = detail.get("code"), detail.get("message")
        if isinstance(code, str) and isinstance(message, str):
            raw = detail.get("fields")
            fields = (
                {name: _redact(value, token) for name, value in raw.items() if isinstance(value, str)}
                if isinstance(raw, dict)
                else {}
            )
            return AdminApiError(status, code, _redact(message, token), fields)
    return AdminApiError(status, f"http_{status}", _redact(text if text else f"HTTP {status}", token))


def _unreachable(error: httpx.TransportError, token: str) -> AdminApiError:
    if isinstance(error, httpx.TimeoutException):
        return AdminApiError(0, "timeout", "No answer within the timeout.")
    return AdminApiError(0, "network_error", _redact(str(error) or "the request failed", token))


def _budget(request: httpx.Request) -> float | None:
    """The total time a request may take: the largest phase of its httpx timeout (None: no limit)."""
    phases = request.extensions.get("timeout")
    values = [v for v in phases.values() if v is not None] if isinstance(phases, dict) else []
    return max(values) if values else None


class _Deadline:
    """A total time limit over the phases of one request, on top of httpx's per-phase timeouts.

    httpx times each connect, write and read on its own, so a server that sends a
    byte now and then never trips it. The limit is checked as the body arrives, and
    the read timeout of the chunk to come is cut to what is left.
    """

    def __init__(self, request: httpx.Request) -> None:
        self._request = request
        self._budget = _budget(request)
        self._started = time.monotonic()

    def check(self) -> None:
        """Raises the timeout error when the time is used up; else shortens the next read."""
        if self._budget is None:
            return
        left = self._budget - (time.monotonic() - self._started)
        if left <= 0:
            raise AdminApiError(0, "timeout", "No answer within the timeout.")
        phases = self._request.extensions.get("timeout")
        if isinstance(phases, dict) and phases.get("read") is not None:
            phases["read"] = min(phases["read"], left)


def _settled(response: httpx.Response, request: httpx.Request, body: bytes, token: str) -> httpx.Response:
    if response.status_code >= 400:
        raise _error_of(response.status_code, body, token)
    return httpx.Response(
        response.status_code,
        headers=response.headers,
        content=body,
        request=request,
        extensions=response.extensions,
    )


class _Guard(httpx.BaseTransport):
    """Makes every failure of a call an ``AdminApiError``, below the generated code.

    That covers a failure to reach the gateway, one while its body arrives, a total
    deadline, and any 4xx/5xx answer whatever its body is (the generated parsers
    would raise a JSON error for a proxy's HTML page). The body is read here, so
    every generated function (``sync``, ``sync_detailed``, ``asyncio``, ...) fails
    the same way.
    """

    def __init__(self, inner: httpx.BaseTransport, token: str) -> None:
        self._inner, self._token = inner, token

    def handle_request(self, request: httpx.Request) -> httpx.Response:
        deadline = _Deadline(request)
        try:
            response = self._inner.handle_request(request)
            try:
                body = bytearray()
                deadline.check()
                for chunk in response.stream:  # type: ignore[union-attr]
                    body += chunk
                    deadline.check()
            finally:
                response.close()
        except httpx.TransportError as error:
            raise _unreachable(error, self._token) from None
        return _settled(response, request, bytes(body), self._token)

    def close(self) -> None:
        self._inner.close()


class _AsyncGuard(httpx.AsyncBaseTransport):
    def __init__(self, inner: httpx.AsyncBaseTransport, token: str) -> None:
        self._inner, self._token = inner, token

    async def handle_async_request(self, request: httpx.Request) -> httpx.Response:
        deadline = _Deadline(request)
        try:
            response = await self._inner.handle_async_request(request)
            try:
                body = bytearray()
                deadline.check()
                async for chunk in response.stream:  # type: ignore[union-attr]
                    body += chunk
                    deadline.check()
            finally:
                await response.aclose()
        except httpx.TransportError as error:
            raise _unreachable(error, self._token) from None
        return _settled(response, request, bytes(body), self._token)

    async def aclose(self) -> None:
        await self._inner.aclose()


class _HttpClient(httpx.Client):
    """Maps what is raised above the transport: use after ``close()``."""

    def send(self, request: httpx.Request, **kwargs: Any) -> httpx.Response:
        try:
            return super().send(request, **kwargs)
        except RuntimeError:
            if self.is_closed:
                raise AdminApiError(0, "network_error", "The client is closed.") from None
            raise


class _AsyncHttpClient(httpx.AsyncClient):
    async def send(self, request: httpx.Request, **kwargs: Any) -> httpx.Response:
        try:
            return await super().send(request, **kwargs)
        except RuntimeError:
            if self.is_closed:
                raise AdminApiError(0, "network_error", "The client is closed.") from None
            raise


_Transports = Callable[[], tuple[httpx.BaseTransport, httpx.AsyncBaseTransport]]


class _Client(AuthenticatedClient):
    """The generated client, with guarded httpx clients and a representation that never shows the token.

    ``with_headers``, ``with_cookies`` and ``with_timeout`` return a copy that is
    guarded the same way, with the bearer header and the same transport kind (the
    generated versions would build a plain httpx client). The original is not changed.
    """

    _transports: _Transports
    _timeout_seconds: httpx.Timeout

    def __repr__(self) -> str:
        return f"AuthenticatedClient(base_url={self._base_url!r})"

    __str__ = __repr__

    def with_headers(self, headers: dict[str, str]) -> Self:
        return _make_client(
            self._base_url,
            self.token,
            self._transports,
            self._timeout_seconds,
            {**self._headers, **headers},
            self._cookies,
        )  # type: ignore[return-value]

    def with_cookies(self, cookies: dict[str, str]) -> Self:
        return _make_client(
            self._base_url,
            self.token,
            self._transports,
            self._timeout_seconds,
            self._headers,
            {**self._cookies, **cookies},
        )  # type: ignore[return-value]

    def with_timeout(self, timeout: httpx.Timeout) -> Self:
        return _make_client(self._base_url, self.token, self._transports, timeout, self._headers, self._cookies)  # type: ignore[return-value]


def _make_client(
    base: str,
    token: str,
    transports: _Transports,
    timeout: httpx.Timeout,
    headers: Mapping[str, str],
    cookies: Mapping[str, str],
) -> _Client:
    client = _Client(base_url=base, token=token, timeout=timeout, headers=dict(headers), cookies=dict(cookies))
    client._transports = transports
    client._timeout_seconds = timeout
    sync_inner, async_inner = transports()
    sent = {"Authorization": f"Bearer {token}", **headers}
    client.set_httpx_client(
        _HttpClient(
            base_url=base, headers=sent, cookies=dict(cookies), timeout=timeout, transport=_Guard(sync_inner, token)
        )
    )
    client.set_async_httpx_client(
        _AsyncHttpClient(
            base_url=base,
            headers=sent,
            cookies=dict(cookies),
            timeout=timeout,
            transport=_AsyncGuard(async_inner, token),
        )
    )
    return client


class AdminClient:
    """The admin API of one gateway, with an access token (Account, Access tokens).

    ``client`` is the generated ``AuthenticatedClient`` to pass to every call.
    ``timeout`` is per request, in seconds, and is a total: connecting, sending and
    receiving the whole answer must fit in it. A call that runs out raises
    ``AdminApiError`` with status 0 and code ``timeout``. ``transport`` replaces the
    network (for tests): an ``httpx.BaseTransport`` that may also be async.
    """

    def __init__(
        self,
        base_url: str,
        token: str,
        timeout: float = DEFAULT_TIMEOUT,
        *,
        transport: httpx.BaseTransport | httpx.AsyncBaseTransport | None = None,
    ) -> None:
        def transports() -> tuple[httpx.BaseTransport, httpx.AsyncBaseTransport]:
            return (
                transport if isinstance(transport, httpx.BaseTransport) else httpx.HTTPTransport(),
                transport if isinstance(transport, httpx.AsyncBaseTransport) else httpx.AsyncHTTPTransport(),
            )

        self.client: AuthenticatedClient = _make_client(
            base_url.rstrip("/"), token, transports, httpx.Timeout(timeout), {}, {}
        )
        self._token = token

    def __repr__(self) -> str:
        return f"AdminClient(base_url={self.client._base_url!r})"

    def __reduce__(self) -> NoReturn:
        raise TypeError("an AdminClient holds a token and is not pickled")

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    async def __aenter__(self) -> Self:
        return self

    async def __aexit__(self, *exc: object) -> None:
        await self.aclose()

    def close(self) -> None:
        self.client.get_httpx_client().close()

    async def aclose(self) -> None:
        await self.client.get_async_httpx_client().aclose()

    def call(self, response: Response[Any]) -> Any:
        """Returns the parsed body of a ``sync_detailed``/``asyncio_detailed`` result; a 204 gives ``None``.

        A failed call has already raised an ``AdminApiError`` (see ``_Guard``);
        this raises one for any other status that is not a success.
        """
        if not response.status_code.is_success:
            raise _error_of(int(response.status_code), response.content, self._token)
        return response.parsed

    def download_backup(self, timeout: float | None = None) -> bytes:
        """The database as the bytes of a SQLite file (``GET /api/backup``).

        ``timeout`` (seconds) replaces the client's for this download.
        """
        return self.client.get_httpx_client().get("/api/backup", timeout=_per_call(timeout)).content

    async def adownload_backup(self, timeout: float | None = None) -> bytes:
        return (await self.client.get_async_httpx_client().get("/api/backup", timeout=_per_call(timeout))).content

    def export_config(self, timeout: float | None = None) -> dict[str, Any]:
        """The configuration file (``GET /api/config/export``), parsed."""
        response = self.client.get_httpx_client().get("/api/config/export", timeout=_per_call(timeout))
        data: dict[str, Any] = response.json()
        return data

    async def aexport_config(self, timeout: float | None = None) -> dict[str, Any]:
        response = await self.client.get_async_httpx_client().get("/api/config/export", timeout=_per_call(timeout))
        data: dict[str, Any] = response.json()
        return data


def _per_call(timeout: float | None) -> Any:
    return httpx.USE_CLIENT_DEFAULT if timeout is None else httpx.Timeout(timeout)
