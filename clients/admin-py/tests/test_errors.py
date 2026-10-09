from __future__ import annotations

from collections.abc import Callable
from typing import Any

import pytest

from ultrafast_admin import AdminApiError, AdminClient
from ultrafast_admin._generated.api.keys import keys_view
from ultrafast_admin._generated.api.providers import providers_create, providers_list
from ultrafast_admin._generated.models import CreateProviderRequest

from .gateway import Gateway


def failure(call: Callable[[], Any]) -> AdminApiError:
    with pytest.raises(AdminApiError) as raised:
        call()
    return raised.value


def test_401_a_token_the_gateway_does_not_know(gateway: Gateway) -> None:
    bad = "uf-at-not-a-real-token"
    api = AdminClient(gateway.origin, bad)
    error = failure(lambda: api.call(providers_list.sync_detailed(client=api.client)))
    assert error.status == 401
    assert error.code == "unauthenticated"
    assert error.fields == {}
    assert bad not in error.message
    assert bad not in str(error)
    assert bad not in repr(error)


def test_403_a_member_may_not_add_a_provider(gateway: Gateway, member: str) -> None:
    api = AdminClient(gateway.origin, member)
    body = CreateProviderRequest(name="p", kind="openai", base_url="http://127.0.0.1:1")
    error = failure(lambda: api.call(providers_create.sync_detailed(client=api.client, body=body)))
    assert (error.status, error.code) == (403, "forbidden")


def test_404_a_key_that_does_not_exist(api: AdminClient) -> None:
    error = failure(lambda: api.call(keys_view.sync_detailed(999999, client=api.client)))
    assert (error.status, error.code) == (404, "not_found")


def test_409_a_provider_name_taken(api: AdminClient) -> None:
    body = CreateProviderRequest(name="dup", kind="openai", base_url="http://127.0.0.1:1")
    api.call(providers_create.sync_detailed(client=api.client, body=body))
    error = failure(lambda: api.call(providers_create.sync_detailed(client=api.client, body=body)))
    assert (error.status, error.code) == (409, "provider_exists")


def test_422_validation_fields(api: AdminClient) -> None:
    body = CreateProviderRequest(name="ok", kind="nonsense", base_url="http://127.0.0.1:1")
    error = failure(lambda: api.call(providers_create.sync_detailed(client=api.client, body=body)))
    assert (error.status, error.code) == (422, "validation_failed")
    assert "kind" in error.fields
    assert "kind must be" in error.fields["kind"]


def test_a_gateway_that_is_not_there_is_a_typed_error_with_status_0(admin_token: str) -> None:
    api = AdminClient("http://127.0.0.1:1", admin_token)
    error = failure(lambda: api.call(providers_list.sync_detailed(client=api.client)))
    assert (error.status, error.code) == (0, "network_error")
    assert admin_token not in error.message


async def test_the_same_mapping_for_async_calls(gateway: Gateway) -> None:
    api = AdminClient("http://127.0.0.1:1", "uf-at-x")
    with pytest.raises(AdminApiError) as raised:
        await providers_list.asyncio_detailed(client=api.client)
    assert (raised.value.status, raised.value.code) == (0, "network_error")
    await api.aclose()
