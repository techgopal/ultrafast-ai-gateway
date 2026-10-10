from __future__ import annotations

from collections.abc import Iterator

import pytest

from ultrafast_admin import AdminClient

from .gateway import Gateway, start_gateway
from .session import member_token, mint_token


@pytest.fixture(scope="module")
def gateway() -> Iterator[Gateway]:
    started = start_gateway()
    try:
        yield started
    finally:
        started.stop()


@pytest.fixture(scope="module")
def admin_token(gateway: Gateway) -> str:
    return mint_token(gateway, gateway.admin)


@pytest.fixture(scope="module")
def api(gateway: Gateway, admin_token: str) -> Iterator[AdminClient]:
    client = AdminClient(gateway.origin, admin_token)
    yield client
    client.close()


@pytest.fixture(scope="module")
def member(gateway: Gateway, admin_token: str) -> str:
    return member_token(gateway, admin_token, "member@example.com")
