from __future__ import annotations

import httpx
import pytest
from ultrafast_admin.api.providers import providers_list
from ultrafast_admin.models import CreateProviderRequest

import ultrafast_admin
from ultrafast_admin import AdminApiError, AdminClient, api, models
from ultrafast_admin._generated import api as generated_api
from ultrafast_admin._generated import models as generated_models


def test_api_and_models_are_the_generated_packages() -> None:
    assert api is generated_api
    assert models is generated_models
    assert ultrafast_admin.api is generated_api
    assert providers_list.sync_detailed is not None
    assert CreateProviderRequest is generated_models.CreateProviderRequest


def _html(request: httpx.Request) -> httpx.Response:
    return httpx.Response(200, content=b"<html>uf-at-secret</html>")


def test_a_200_export_that_is_not_json_is_an_admin_api_error() -> None:
    client = AdminClient("http://gw.test", "uf-at-secret", transport=httpx.MockTransport(_html))
    with pytest.raises(AdminApiError) as raised:
        client.export_config()
    assert (raised.value.status, raised.value.code) == (200, "invalid_response")
    assert "html" not in raised.value.message


async def test_the_same_for_async_export() -> None:
    client = AdminClient("http://gw.test", "uf-at-secret", transport=httpx.MockTransport(_html))
    with pytest.raises(AdminApiError) as raised:
        await client.aexport_config()
    assert raised.value.code == "invalid_response"
    await client.aclose()
