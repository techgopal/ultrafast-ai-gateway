"""Timeouts, copies of the client, and use after close, against a server that misbehaves."""

from __future__ import annotations

import time

import httpx
import pytest

from ultrafast_admin import AdminApiError, AdminClient
from ultrafast_admin._generated.api.providers import providers_list

from .slow_server import slow_server

TOKEN = "uf-at-secret-value-1234567890"


def timeout_error(call: object) -> tuple[AdminApiError, float]:
    started = time.monotonic()
    with pytest.raises(AdminApiError) as raised:
        call()  # type: ignore[operator]
    return raised.value, time.monotonic() - started


@pytest.mark.parametrize("status", [200, 500])
def test_a_body_that_stalls_is_a_timeout_error(status: int) -> None:
    with slow_server("stall", status) as url:
        api = AdminClient(url, TOKEN, timeout=0.5)
        error, took = timeout_error(lambda: providers_list.sync_detailed(client=api.client))
    assert (error.status, error.code) == (0, "timeout")
    assert took < 3


@pytest.mark.parametrize("status", [200, 502])
async def test_the_same_for_async_calls(status: int) -> None:
    with slow_server("stall", status) as url:
        api = AdminClient(url, TOKEN, timeout=0.5)
        with pytest.raises(AdminApiError) as raised:
            await providers_list.asyncio_detailed(client=api.client)
        await api.aclose()
    assert (raised.value.status, raised.value.code) == (0, "timeout")


def test_no_answer_at_all_is_a_timeout_error() -> None:
    with slow_server("silent") as url:
        api = AdminClient(url, TOKEN, timeout=0.4)
        error, took = timeout_error(lambda: providers_list.sync_detailed(client=api.client))
    assert (error.status, error.code) == (0, "timeout")
    assert took < 3


def test_the_timeout_is_a_total_deadline_not_per_read() -> None:
    # A byte every 0.1 s never trips a per-read timeout of 0.5 s.
    with slow_server("drip") as url:
        api = AdminClient(url, TOKEN, timeout=0.6)
        error, took = timeout_error(lambda: providers_list.sync_detailed(client=api.client))
    assert (error.status, error.code) == (0, "timeout")
    assert took < 2


async def test_the_total_deadline_for_async_calls() -> None:
    with slow_server("drip") as url:
        api = AdminClient(url, TOKEN, timeout=0.6)
        started = time.monotonic()
        with pytest.raises(AdminApiError) as raised:
            await providers_list.asyncio_detailed(client=api.client)
        await api.aclose()
    assert raised.value.code == "timeout"
    assert time.monotonic() - started < 2


def test_with_timeout_keeps_the_guard_and_applies_the_new_timeout() -> None:
    with slow_server("stall") as url:
        api = AdminClient(url, TOKEN, timeout=30.0)
        shorter = api.client.with_timeout(httpx.Timeout(0.4))
        error, took = timeout_error(lambda: providers_list.sync_detailed(client=shorter))
    assert (error.status, error.code) == (0, "timeout")
    assert took < 3


def test_with_headers_and_with_cookies_keep_the_bearer_the_guard_and_the_transport() -> None:
    seen: list[httpx.Request] = []

    def handle(request: httpx.Request) -> httpx.Response:
        seen.append(request)
        if request.url.path == "/api/providers" and "x-extra" in request.headers:
            return httpx.Response(200, json={"providers": []})
        return httpx.Response(403, json={"error": {"code": "forbidden", "message": f"no {TOKEN}"}})

    api = AdminClient("http://gw.test", TOKEN, transport=httpx.MockTransport(handle))
    copy = api.client.with_headers({"x-extra": "1"}).with_cookies({"a": "b"})
    assert api.call(providers_list.sync_detailed(client=copy)).providers == []
    assert seen[0].headers["authorization"] == f"Bearer {TOKEN}"
    assert "a=b" in seen[0].headers["cookie"]
    # Without the extra header the gateway answers 403: typed and redacted.
    plain = api.client.with_headers({})
    with pytest.raises(AdminApiError) as raised:
        providers_list.sync_detailed(client=plain)
    assert raised.value.code == "forbidden"
    assert TOKEN not in raised.value.message
    assert TOKEN not in repr(copy) + str(copy)


def test_a_copy_of_an_unreachable_client_fails_typed() -> None:
    api = AdminClient("http://127.0.0.1:1", TOKEN)
    for copy in (api.client.with_timeout(httpx.Timeout(2.0)), api.client.with_headers({"x": "y"})):
        with pytest.raises(AdminApiError) as raised:
            providers_list.sync_detailed(client=copy)
        assert raised.value.code == "network_error"


def test_use_after_close_is_a_typed_error() -> None:
    api = AdminClient("http://127.0.0.1:1", TOKEN)
    api.close()
    with pytest.raises(AdminApiError) as raised:
        providers_list.sync_detailed(client=api.client)
    assert raised.value.status == 0


async def test_use_after_aclose_is_a_typed_error() -> None:
    api = AdminClient("http://127.0.0.1:1", TOKEN)
    await api.aclose()
    with pytest.raises(AdminApiError) as raised:
        await providers_list.asyncio_detailed(client=api.client)
    assert raised.value.status == 0


def test_a_download_can_take_its_own_timeout() -> None:
    with slow_server("stall") as url:
        api = AdminClient(url, TOKEN, timeout=30.0)
        error, took = timeout_error(lambda: api.download_backup(timeout=0.4))
        assert (error.status, error.code) == (0, "timeout")
        error, took = timeout_error(lambda: api.export_config(timeout=0.4))
        assert error.code == "timeout"
    assert took < 3
