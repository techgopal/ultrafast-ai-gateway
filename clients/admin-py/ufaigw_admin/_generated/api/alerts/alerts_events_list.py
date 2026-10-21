from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error_body import ApiErrorBody
from ...models.event_list import EventList
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    rule_id: int | Unset = UNSET,
    state: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    before_id: int | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["rule_id"] = rule_id

    params["state"] = state

    params["limit"] = limit

    params["before_id"] = before_id

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/alerts/events",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ApiErrorBody | EventList | None:
    if response.status_code == 200:
        response_200 = EventList.from_dict(response.json())

        return response_200

    if response.status_code == 400:
        response_400 = ApiErrorBody.from_dict(response.json())

        return response_400

    if response.status_code == 401:
        response_401 = ApiErrorBody.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = ApiErrorBody.from_dict(response.json())

        return response_403

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
) -> Response[ApiErrorBody | EventList]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    rule_id: int | Unset = UNSET,
    state: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    before_id: int | Unset = UNSET,
) -> Response[ApiErrorBody | EventList]:
    """
    Args:
        rule_id (int | Unset):
        state (str | Unset):
        limit (int | Unset):
        before_id (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ApiErrorBody | EventList]
    """

    kwargs = _get_kwargs(
        rule_id=rule_id,
        state=state,
        limit=limit,
        before_id=before_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
    rule_id: int | Unset = UNSET,
    state: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    before_id: int | Unset = UNSET,
) -> ApiErrorBody | EventList | None:
    """
    Args:
        rule_id (int | Unset):
        state (str | Unset):
        limit (int | Unset):
        before_id (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ApiErrorBody | EventList
    """

    return sync_detailed(
        client=client,
        rule_id=rule_id,
        state=state,
        limit=limit,
        before_id=before_id,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    rule_id: int | Unset = UNSET,
    state: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    before_id: int | Unset = UNSET,
) -> Response[ApiErrorBody | EventList]:
    """
    Args:
        rule_id (int | Unset):
        state (str | Unset):
        limit (int | Unset):
        before_id (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ApiErrorBody | EventList]
    """

    kwargs = _get_kwargs(
        rule_id=rule_id,
        state=state,
        limit=limit,
        before_id=before_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    rule_id: int | Unset = UNSET,
    state: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    before_id: int | Unset = UNSET,
) -> ApiErrorBody | EventList | None:
    """
    Args:
        rule_id (int | Unset):
        state (str | Unset):
        limit (int | Unset):
        before_id (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ApiErrorBody | EventList
    """

    return (
        await asyncio_detailed(
            client=client,
            rule_id=rule_id,
            state=state,
            limit=limit,
            before_id=before_id,
        )
    ).parsed
