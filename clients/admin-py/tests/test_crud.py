"""Round trips through the token, one per tag, in order (later tests use earlier ids)."""

from __future__ import annotations

import json
from typing import Any

import pytest

from ultrafast_admin import AdminApiError, AdminClient
from ultrafast_admin._generated.api.alerts import (
    alerts_channels_create,
    alerts_channels_delete,
    alerts_rules_create,
    alerts_rules_delete,
    alerts_rules_list,
)
from ultrafast_admin._generated.api.budgets import budgets_delete, budgets_list, budgets_set
from ultrafast_admin._generated.api.keys import keys_create, keys_list, keys_revoke, keys_view
from ultrafast_admin._generated.api.limits import limits_delete, limits_list, limits_set
from ultrafast_admin._generated.api.models import models_create, models_delete, models_list, models_update
from ultrafast_admin._generated.api.providers import (
    providers_create,
    providers_delete,
    providers_list,
    providers_update,
)
from ultrafast_admin._generated.api.teams import teams_create, teams_delete, teams_rename, teams_view
from ultrafast_admin._generated.api.tokens import tokens_list, tokens_revoke
from ultrafast_admin._generated.api.users import users_invite, users_list
from ultrafast_admin._generated.models import (
    CreateChannelRequest,
    CreateKeyRequest,
    CreateModelRequest,
    CreateProviderRequest,
    CreateRuleRequest,
    CreateRuleRequestParams,
    InviteRequest,
    SetBudgetRequest,
    SetLimitRequest,
    TeamNameRequest,
    UpdateModelRequest,
    UpdateProviderRequest,
)

from .gateway import Gateway
from .session import mint_token

ids: dict[str, Any] = {}


def test_providers_create_list_update(api: AdminClient) -> None:
    created = api.call(
        providers_create.sync_detailed(
            client=api.client,
            body=CreateProviderRequest(
                name="local", kind="openai", base_url="http://127.0.0.1:1", api_key="sk-test-not-real"
            ),
        )
    )
    ids["provider"] = created.id
    assert created.has_credential is True
    assert "sk-test-not-real" not in json.dumps(created.to_dict())
    updated = api.call(
        providers_update.sync_detailed(
            created.id, client=api.client, body=UpdateProviderRequest(base_url="http://127.0.0.1:1/v1")
        )
    )
    assert updated.base_url == "http://127.0.0.1:1/v1"
    listed = api.call(providers_list.sync_detailed(client=api.client))
    assert [p.name for p in listed.providers] == ["local"]


def test_models_create_enable_list(api: AdminClient) -> None:
    created = api.call(
        models_create.sync_detailed(
            client=api.client, body=CreateModelRequest(provider_id=ids["provider"], name="gpt-test")
        )
    )
    ids["model"] = created.id
    assert created.enabled is False
    updated = api.call(
        models_update.sync_detailed(
            created.id, client=api.client, body=UpdateModelRequest(enabled=True, input_price_micros=1000)
        )
    )
    assert updated.enabled is True
    listed = api.call(models_list.sync_detailed(client=api.client))
    assert "gpt-test" in [m.name for m in listed.models]


def test_keys_secret_once_then_revoke(api: AdminClient) -> None:
    created = api.call(keys_create.sync_detailed(client=api.client, body=CreateKeyRequest(name="ci")))
    assert created.secret.startswith("uf-")
    listed = api.call(keys_list.sync_detailed(client=api.client))
    assert created.secret not in json.dumps(listed.to_dict())
    assert "ci" in [k.name for k in listed.keys]
    api.call(keys_revoke.sync_detailed(created.key.id, client=api.client))
    viewed = api.call(keys_view.sync_detailed(created.key.id, client=api.client))
    assert viewed.revoked_at is not None


def test_budgets_set_list_delete(api: AdminClient) -> None:
    body = SetBudgetRequest(scope="gateway", amount_micros=5_000_000, period="monthly", action="alert")
    saved = api.call(budgets_set.sync_detailed(client=api.client, body=body))
    assert saved.amount_micros == 5_000_000
    listed = api.call(budgets_list.sync_detailed(client=api.client))
    assert saved.id in [b.id for b in listed.budgets]
    assert api.call(budgets_delete.sync_detailed(saved.id, client=api.client)) is None


def test_limits_set_delete(api: AdminClient) -> None:
    saved = api.call(
        limits_set.sync_detailed(client=api.client, body=SetLimitRequest(scope="gateway", requests_per_minute=600))
    )
    assert saved.requests_per_minute == 600
    api.call(limits_delete.sync_detailed(saved.id, client=api.client))
    assert api.call(limits_list.sync_detailed(client=api.client)).limits == []


def test_teams_create_rename_view_delete(api: AdminClient) -> None:
    created = api.call(teams_create.sync_detailed(client=api.client, body=TeamNameRequest(name="Platform")))
    api.call(teams_rename.sync_detailed(created.id, client=api.client, body=TeamNameRequest(name="Infra")))
    viewed = api.call(teams_view.sync_detailed(created.id, client=api.client))
    assert viewed.team.name == "Infra"
    api.call(teams_delete.sync_detailed(created.id, client=api.client))


def test_alerts_channel_then_rule_then_removed(api: AdminClient) -> None:
    channel = api.call(
        alerts_channels_create.sync_detailed(
            client=api.client, body=CreateChannelRequest(name="ops", kind="webhook", url="http://127.0.0.1:9/hook")
        )
    )
    # `params` is an open object: the generated model keeps what it is given as
    # additional properties, and the gateway reads it by `kind`.
    params = CreateRuleRequestParams.from_dict({"scope": "gateway", "percent": 50})
    rule = api.call(
        alerts_rules_create.sync_detailed(
            client=api.client,
            body=CreateRuleRequest(name="errors", kind="error_rate", params=params, channel_ids=[channel.channel.id]),
        )
    )
    assert rule.kind == "error_rate"
    rules = api.call(alerts_rules_list.sync_detailed(client=api.client))
    assert "errors" in [r.name for r in rules.rules]
    api.call(alerts_rules_delete.sync_detailed(rule.id, client=api.client))
    api.call(alerts_channels_delete.sync_detailed(channel.channel.id, client=api.client))


def test_tokens_list_revoke_and_revoked_is_refused(api: AdminClient, gateway: Gateway) -> None:
    second = mint_token(gateway, gateway.admin, "second")
    listed = api.call(tokens_list.sync_detailed(client=api.client))
    assert "second" in [t.name for t in listed.tokens]
    assert second not in json.dumps(listed.to_dict())
    token_id = next(t.id for t in listed.tokens if t.name == "second")
    api.call(tokens_revoke.sync_detailed(token_id, client=api.client))
    other = AdminClient(gateway.origin, second)
    with pytest.raises(AdminApiError) as refused:
        other.call(tokens_list.sync_detailed(client=other.client))
    assert refused.value.status == 401
    other.close()


def test_users_invite_and_list(api: AdminClient) -> None:
    invited = api.call(
        users_invite.sync_detailed(
            client=api.client, body=InviteRequest(email="new@example.com", name="New", role="member")
        )
    )
    assert invited.invite_link
    listed = api.call(users_list.sync_detailed(client=api.client))
    assert "new@example.com" in [u.email for u in listed.users]


def test_models_and_providers_delete_in_order(api: AdminClient) -> None:
    api.call(models_delete.sync_detailed(ids["model"], client=api.client))
    api.call(providers_delete.sync_detailed(ids["provider"], client=api.client))
    assert api.call(providers_list.sync_detailed(client=api.client)).providers == []


async def test_async_calls_work_the_same(api: AdminClient) -> None:
    response = await providers_list.asyncio_detailed(client=api.client)
    assert api.call(response).providers == []
    created = api.call(await teams_create.asyncio_detailed(client=api.client, body=TeamNameRequest(name="Async")))
    api.call(await teams_delete.asyncio_detailed(created.id, client=api.client))
    await api.aclose()
