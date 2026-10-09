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
from collections.abc import Mapping
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


def _unreachable(error: httpx.TransportError, token: str, request: httpx.Request | None) -> AdminApiError:
    if isinstance(error, httpx.TimeoutException):
        return AdminApiError(0, "timeout", "No answer within the timeout.")
    return AdminApiError(0, "network_error", _redact(str(error) or "the request failed", token))


class _Guard(httpx.BaseTransport):
    """Turns a failure to reach the gateway, and any 4xx/5xx answer, into an ``AdminApiError``.

    Done below the generated code so that every function (``sync``,
    ``sync_detailed``, ``asyncio``, ...) fails the same way, whatever the body of
    the answer is: the generated parsers would otherwise raise a JSON error for a
    proxy's HTML page.
    """

    def __init__(self, inner: httpx.BaseTransport, token: str) -> None:
        self._inner, self._token = inner, token

    def handle_request(self, request: httpx.Request) -> httpx.Response:
        try:
            response = self._inner.handle_request(request)
        except httpx.TransportError as error:
            raise _unreachable(error, self._token, request) from None
        if response.status_code >= 400:
            try:
                response.read()
            finally:
                response.close()
            raise _error_of(response.status_code, response.content, self._token)
        return response

    def close(self) -> None:
        self._inner.close()


class _AsyncGuard(httpx.AsyncBaseTransport):
    def __init__(self, inner: httpx.AsyncBaseTransport, token: str) -> None:
        self._inner, self._token = inner, token

    async def handle_async_request(self, request: httpx.Request) -> httpx.Response:
        try:
            response = await self._inner.handle_async_request(request)
        except httpx.TransportError as error:
            raise _unreachable(error, self._token, request) from None
        if response.status_code >= 400:
            try:
                await response.aread()
            finally:
                await response.aclose()
            raise _error_of(response.status_code, response.content, self._token)
        return response

    async def aclose(self) -> None:
        await self._inner.aclose()


class _Client(AuthenticatedClient):
    """The generated client, whose representation never shows the token."""

    def __repr__(self) -> str:
        return f"AuthenticatedClient(base_url={self._base_url!r})"

    __str__ = __repr__


class AdminClient:
    """The admin API of one gateway, with an access token (Account, Access tokens).

    ``client`` is the generated ``AuthenticatedClient`` to pass to every call.
    ``timeout`` is per request, in seconds. ``transport`` replaces the network
    (for tests): an ``httpx.BaseTransport`` that may also be async.
    """

    def __init__(
        self,
        base_url: str,
        token: str,
        timeout: float = DEFAULT_TIMEOUT,
        *,
        transport: httpx.BaseTransport | httpx.AsyncBaseTransport | None = None,
    ) -> None:
        base = base_url.rstrip("/")
        limit = httpx.Timeout(timeout)
        headers = {"Authorization": f"Bearer {token}"}
        sync_inner = transport if isinstance(transport, httpx.BaseTransport) else httpx.HTTPTransport()
        async_inner = transport if isinstance(transport, httpx.AsyncBaseTransport) else httpx.AsyncHTTPTransport()
        client = _Client(base_url=base, token=token, timeout=limit)
        client.set_httpx_client(
            httpx.Client(base_url=base, headers=headers, timeout=limit, transport=_Guard(sync_inner, token))
        )
        client.set_async_httpx_client(
            httpx.AsyncClient(base_url=base, headers=headers, timeout=limit, transport=_AsyncGuard(async_inner, token))
        )
        self.client: AuthenticatedClient = client
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

    def _checked(self, response: httpx.Response) -> httpx.Response:
        if not response.is_success:
            raise _error_of(response.status_code, response.content, self._token)
        return response

    def download_backup(self) -> bytes:
        """The database as the bytes of a SQLite file (``GET /api/backup``)."""
        return self._checked(self.client.get_httpx_client().get("/api/backup")).content

    async def adownload_backup(self) -> bytes:
        return self._checked(await self.client.get_async_httpx_client().get("/api/backup")).content

    def export_config(self) -> dict[str, Any]:
        """The configuration file (``GET /api/config/export``), parsed."""
        data: dict[str, Any] = self._checked(self.client.get_httpx_client().get("/api/config/export")).json()
        return data

    async def aexport_config(self) -> dict[str, Any]:
        data: dict[str, Any] = self._checked(
            await self.client.get_async_httpx_client().get("/api/config/export")
        ).json()
        return data
