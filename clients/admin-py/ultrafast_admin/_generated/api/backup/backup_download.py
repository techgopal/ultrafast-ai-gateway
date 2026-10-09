from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error_body import ApiErrorBody
from ...types import Response


def _get_kwargs() -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/backup",
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ApiErrorBody | None:
    if response.status_code == 401:
        response_401 = ApiErrorBody.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = ApiErrorBody.from_dict(response.json())

        return response_403

    if response.status_code == 409:
        response_409 = ApiErrorBody.from_dict(response.json())

        return response_409

    if response.status_code == 500:
        response_500 = ApiErrorBody.from_dict(response.json())

        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ApiErrorBody]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
) -> Response[ApiErrorBody]:
    """Downloads a consistent copy of the database: a SQLite file with every
    table, taken of one moment while the gateway goes on. It holds
    everything the database holds (users with their password hashes,
    sessions, logs, provider credentials as they are stored) except the
    master key, which is not in it: the credentials are unreadable without
    that key, and so the copy is of little use without it. Admin only; the
    download is audited. On PostgreSQL there is no file to give: 409
    `backup_unsupported`, with the advice to use `pg_dump`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ApiErrorBody]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
) -> ApiErrorBody | None:
    """Downloads a consistent copy of the database: a SQLite file with every
    table, taken of one moment while the gateway goes on. It holds
    everything the database holds (users with their password hashes,
    sessions, logs, provider credentials as they are stored) except the
    master key, which is not in it: the credentials are unreadable without
    that key, and so the copy is of little use without it. Admin only; the
    download is audited. On PostgreSQL there is no file to give: 409
    `backup_unsupported`, with the advice to use `pg_dump`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ApiErrorBody
    """

    return sync_detailed(
        client=client,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
) -> Response[ApiErrorBody]:
    """Downloads a consistent copy of the database: a SQLite file with every
    table, taken of one moment while the gateway goes on. It holds
    everything the database holds (users with their password hashes,
    sessions, logs, provider credentials as they are stored) except the
    master key, which is not in it: the credentials are unreadable without
    that key, and so the copy is of little use without it. Admin only; the
    download is audited. On PostgreSQL there is no file to give: 409
    `backup_unsupported`, with the advice to use `pg_dump`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ApiErrorBody]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
) -> ApiErrorBody | None:
    """Downloads a consistent copy of the database: a SQLite file with every
    table, taken of one moment while the gateway goes on. It holds
    everything the database holds (users with their password hashes,
    sessions, logs, provider credentials as they are stored) except the
    master key, which is not in it: the credentials are unreadable without
    that key, and so the copy is of little use without it. Admin only; the
    download is audited. On PostgreSQL there is no file to give: 409
    `backup_unsupported`, with the advice to use `pg_dump`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ApiErrorBody
    """

    return (
        await asyncio_detailed(
            client=client,
        )
    ).parsed
