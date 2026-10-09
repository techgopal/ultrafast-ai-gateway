from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error_body import ApiErrorBody
from ...models.key_view import KeyView
from ...models.update_key_request import UpdateKeyRequest
from ...types import UNSET, Response, Unset


def _get_kwargs(
    id: int,
    *,
    body: UpdateKeyRequest,
    x_csrf_token: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_csrf_token, Unset):
        headers["x-csrf-token"] = x_csrf_token

    _kwargs: dict[str, Any] = {
        "method": "patch",
        "url": "/api/keys/{id}".format(
            id=quote(str(id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ApiErrorBody | KeyView | None:
    if response.status_code == 200:
        response_200 = KeyView.from_dict(response.json())

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

    if response.status_code == 404:
        response_404 = ApiErrorBody.from_dict(response.json())

        return response_404

    if response.status_code == 413:
        response_413 = ApiErrorBody.from_dict(response.json())

        return response_413

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
) -> Response[ApiErrorBody | KeyView]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    id: int,
    *,
    client: AuthenticatedClient,
    body: UpdateKeyRequest,
    x_csrf_token: str | Unset = UNSET,
) -> Response[ApiErrorBody | KeyView]:
    """Replaces the tags or the guardrails of a key. Admins only: the key's tags
    win over a call's, and guardrails are the admin's.

    Args:
        id (int):
        x_csrf_token (str | Unset):
        body (UpdateKeyRequest): What to change on a key. Admins only; send at least one field.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ApiErrorBody | KeyView]
    """

    kwargs = _get_kwargs(
        id=id,
        body=body,
        x_csrf_token=x_csrf_token,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    id: int,
    *,
    client: AuthenticatedClient,
    body: UpdateKeyRequest,
    x_csrf_token: str | Unset = UNSET,
) -> ApiErrorBody | KeyView | None:
    """Replaces the tags or the guardrails of a key. Admins only: the key's tags
    win over a call's, and guardrails are the admin's.

    Args:
        id (int):
        x_csrf_token (str | Unset):
        body (UpdateKeyRequest): What to change on a key. Admins only; send at least one field.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ApiErrorBody | KeyView
    """

    return sync_detailed(
        id=id,
        client=client,
        body=body,
        x_csrf_token=x_csrf_token,
    ).parsed


async def asyncio_detailed(
    id: int,
    *,
    client: AuthenticatedClient,
    body: UpdateKeyRequest,
    x_csrf_token: str | Unset = UNSET,
) -> Response[ApiErrorBody | KeyView]:
    """Replaces the tags or the guardrails of a key. Admins only: the key's tags
    win over a call's, and guardrails are the admin's.

    Args:
        id (int):
        x_csrf_token (str | Unset):
        body (UpdateKeyRequest): What to change on a key. Admins only; send at least one field.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ApiErrorBody | KeyView]
    """

    kwargs = _get_kwargs(
        id=id,
        body=body,
        x_csrf_token=x_csrf_token,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    id: int,
    *,
    client: AuthenticatedClient,
    body: UpdateKeyRequest,
    x_csrf_token: str | Unset = UNSET,
) -> ApiErrorBody | KeyView | None:
    """Replaces the tags or the guardrails of a key. Admins only: the key's tags
    win over a call's, and guardrails are the admin's.

    Args:
        id (int):
        x_csrf_token (str | Unset):
        body (UpdateKeyRequest): What to change on a key. Admins only; send at least one field.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ApiErrorBody | KeyView
    """

    return (
        await asyncio_detailed(
            id=id,
            client=client,
            body=body,
            x_csrf_token=x_csrf_token,
        )
    ).parsed
