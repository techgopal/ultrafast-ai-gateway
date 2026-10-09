from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error_body import ApiErrorBody
from ...models.usage_page import UsagePage
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    group: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["from"] = from_

    params["to"] = to

    params["group"] = group

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/usage",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ApiErrorBody | UsagePage | None:
    if response.status_code == 200:
        response_200 = UsagePage.from_dict(response.json())

        return response_200

    if response.status_code == 400:
        response_400 = ApiErrorBody.from_dict(response.json())

        return response_400

    if response.status_code == 401:
        response_401 = ApiErrorBody.from_dict(response.json())

        return response_401

    if response.status_code == 422:
        response_422 = ApiErrorBody.from_dict(response.json())

        return response_422

    if response.status_code == 500:
        response_500 = ApiErrorBody.from_dict(response.json())

        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ApiErrorBody | UsagePage]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    group: str | Unset = UNSET,
) -> Response[ApiErrorBody | UsagePage]:
    """Sums of the calls in a range of days, by day, model, key, user or team.

     What a caller sees depends on who they are, as in the log list. An admin
    sees every call. A team lead sees the calls of the teams they lead, the
    calls of the users who are members of those teams, and their own. Anyone
    else sees their own calls, so grouping by user or team shows only
    themselves and the teams on their own calls. A lead's scope uses the
    CURRENT membership: a call linked to a team only by its user leaves the
    lead's totals when that user leaves the team. Calls of keys without an
    owner count for admins only. Days are UTC. The range is at most 366 days;
    it defaults to the last 30 days.

    Args:
        from_ (str | Unset):
        to (str | Unset):
        group (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ApiErrorBody | UsagePage]
    """

    kwargs = _get_kwargs(
        from_=from_,
        to=to,
        group=group,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    group: str | Unset = UNSET,
) -> ApiErrorBody | UsagePage | None:
    """Sums of the calls in a range of days, by day, model, key, user or team.

     What a caller sees depends on who they are, as in the log list. An admin
    sees every call. A team lead sees the calls of the teams they lead, the
    calls of the users who are members of those teams, and their own. Anyone
    else sees their own calls, so grouping by user or team shows only
    themselves and the teams on their own calls. A lead's scope uses the
    CURRENT membership: a call linked to a team only by its user leaves the
    lead's totals when that user leaves the team. Calls of keys without an
    owner count for admins only. Days are UTC. The range is at most 366 days;
    it defaults to the last 30 days.

    Args:
        from_ (str | Unset):
        to (str | Unset):
        group (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ApiErrorBody | UsagePage
    """

    return sync_detailed(
        client=client,
        from_=from_,
        to=to,
        group=group,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    group: str | Unset = UNSET,
) -> Response[ApiErrorBody | UsagePage]:
    """Sums of the calls in a range of days, by day, model, key, user or team.

     What a caller sees depends on who they are, as in the log list. An admin
    sees every call. A team lead sees the calls of the teams they lead, the
    calls of the users who are members of those teams, and their own. Anyone
    else sees their own calls, so grouping by user or team shows only
    themselves and the teams on their own calls. A lead's scope uses the
    CURRENT membership: a call linked to a team only by its user leaves the
    lead's totals when that user leaves the team. Calls of keys without an
    owner count for admins only. Days are UTC. The range is at most 366 days;
    it defaults to the last 30 days.

    Args:
        from_ (str | Unset):
        to (str | Unset):
        group (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ApiErrorBody | UsagePage]
    """

    kwargs = _get_kwargs(
        from_=from_,
        to=to,
        group=group,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    group: str | Unset = UNSET,
) -> ApiErrorBody | UsagePage | None:
    """Sums of the calls in a range of days, by day, model, key, user or team.

     What a caller sees depends on who they are, as in the log list. An admin
    sees every call. A team lead sees the calls of the teams they lead, the
    calls of the users who are members of those teams, and their own. Anyone
    else sees their own calls, so grouping by user or team shows only
    themselves and the teams on their own calls. A lead's scope uses the
    CURRENT membership: a call linked to a team only by its user leaves the
    lead's totals when that user leaves the team. Calls of keys without an
    owner count for admins only. Days are UTC. The range is at most 366 days;
    it defaults to the last 30 days.

    Args:
        from_ (str | Unset):
        to (str | Unset):
        group (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ApiErrorBody | UsagePage
    """

    return (
        await asyncio_detailed(
            client=client,
            from_=from_,
            to=to,
            group=group,
        )
    ).parsed
