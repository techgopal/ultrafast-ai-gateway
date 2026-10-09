from __future__ import annotations

import pytest

from ultrafast_admin import AdminClient
from ultrafast_admin._generated.api.providers import providers_create
from ultrafast_admin._generated.models import CreateProviderRequest


@pytest.fixture(scope="module", autouse=True)
def provider(api: AdminClient) -> None:
    api.call(
        providers_create.sync_detailed(
            client=api.client,
            body=CreateProviderRequest(name="exported", kind="openai", base_url="http://127.0.0.1:1"),
        )
    )


def test_export_config_returns_the_parsed_configuration_file(api: AdminClient) -> None:
    file = api.export_config()
    assert isinstance(file, dict)
    assert file["format"] == "ultrafast-config"
    assert file["version"] == 1
    assert "exported" in [p["name"] for p in file["providers"]]


def test_download_backup_returns_the_bytes_of_a_sqlite_file(api: AdminClient) -> None:
    data = api.download_backup()
    assert isinstance(data, bytes)
    assert len(data) > 4096
    assert data[:15] == b"SQLite format 3"


async def test_async_downloads(api: AdminClient) -> None:
    assert (await api.adownload_backup())[:15] == b"SQLite format 3"
    assert (await api.aexport_config())["format"] == "ultrafast-config"
