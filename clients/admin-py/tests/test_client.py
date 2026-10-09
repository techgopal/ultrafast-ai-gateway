"""AdminClient without a gateway: a transport that answers from memory."""

from __future__ import annotations

import json
import pickle

import httpx
import pytest

from ultrafast_admin import AdminApiError, AdminClient
from ultrafast_admin._generated.api.providers import providers_list

TOKEN = "uf-at-secret-value-1234567890"


def answering(handler: httpx.MockTransport) -> AdminClient:
    return AdminClient("http://gw.test/", TOKEN, transport=handler)


def test_sends_the_token_as_a_bearer_and_strips_a_trailing_slash() -> None:
    seen: list[httpx.Request] = []

    def handle(request: httpx.Request) -> httpx.Response:
        seen.append(request)
        return httpx.Response(200, json={"providers": []})

    api = answering(httpx.MockTransport(handle))
    api.call(providers_list.sync_detailed(client=api.client))
    assert str(seen[0].url) == "http://gw.test/api/providers"
    assert seen[0].headers["authorization"] == f"Bearer {TOKEN}"


def test_maps_a_body_that_is_not_the_gateways_error_shape() -> None:
    api = answering(httpx.MockTransport(lambda r: httpx.Response(502, text="upstream down")))
    with pytest.raises(AdminApiError) as raised:
        api.call(providers_list.sync_detailed(client=api.client))
    assert (raised.value.status, raised.value.code, raised.value.message) == (502, "http_502", "upstream down")


def test_a_documented_status_with_a_body_that_is_not_json_is_still_typed() -> None:
    api = answering(httpx.MockTransport(lambda r: httpx.Response(401, text="<html>nope</html>")))
    with pytest.raises(AdminApiError) as raised:
        api.call(providers_list.sync_detailed(client=api.client))
    assert (raised.value.status, raised.value.code) == (401, "http_401")


def test_every_generated_function_fails_the_same_way() -> None:
    api = answering(
        httpx.MockTransport(lambda r: httpx.Response(404, json={"error": {"code": "not_found", "message": "no"}}))
    )
    for call in (providers_list.sync, providers_list.sync_detailed):
        with pytest.raises(AdminApiError) as raised:
            call(client=api.client)
        assert (raised.value.status, raised.value.code) == (404, "not_found")


async def test_async_functions_fail_the_same_way() -> None:
    api = answering(httpx.MockTransport(lambda r: httpx.Response(502, text="bad gateway")))
    with pytest.raises(AdminApiError) as raised:
        await providers_list.asyncio(client=api.client)
    assert (raised.value.status, raised.value.code) == (502, "http_502")
    await api.aclose()


def test_times_out_with_a_typed_error() -> None:
    def slow(request: httpx.Request) -> httpx.Response:
        raise httpx.ReadTimeout("too slow", request=request)

    api = answering(httpx.MockTransport(slow))
    with pytest.raises(AdminApiError) as raised:
        api.call(providers_list.sync_detailed(client=api.client))
    assert (raised.value.status, raised.value.code) == (0, "timeout")


def test_the_timeout_argument_is_the_default_of_30_seconds() -> None:
    assert answering(
        httpx.MockTransport(lambda r: httpx.Response(200))
    ).client.get_httpx_client().timeout == httpx.Timeout(30.0)
    other = AdminClient(
        "http://gw.test", TOKEN, timeout=5.0, transport=httpx.MockTransport(lambda r: httpx.Response(200))
    )
    assert other.client.get_httpx_client().timeout == httpx.Timeout(5.0)


def test_never_shows_the_token_in_the_client_an_error_or_its_representation() -> None:
    def handle(request: httpx.Request) -> httpx.Response:
        return httpx.Response(403, json={"error": {"code": "forbidden", "message": f"bad {TOKEN}"}})

    api = answering(httpx.MockTransport(handle))
    with pytest.raises(AdminApiError) as raised:
        api.call(providers_list.sync_detailed(client=api.client))
    error = raised.value
    for text in (str(error), error.message, repr(error), repr(api), str(api), repr(api.client), str(api.client)):
        assert TOKEN not in text
    assert TOKEN not in json.dumps(error.fields)
    with pytest.raises(TypeError):
        pickle.dumps(api)
