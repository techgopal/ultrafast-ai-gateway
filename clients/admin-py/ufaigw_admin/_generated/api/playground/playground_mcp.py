from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.playground_error_body import PlaygroundErrorBody
from ...models.playground_mcp_error import PlaygroundMcpError
from ...models.playground_mcp_reply import PlaygroundMcpReply
from ...models.playground_mcp_request import PlaygroundMcpRequest
from ...types import UNSET, Response, Unset


def _get_kwargs(
    server: str,
    *,
    body: PlaygroundMcpRequest,
    x_csrf_token: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_csrf_token, Unset):
        headers["x-csrf-token"] = x_csrf_token

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/api/playground/mcp/{server}".format(
            server=quote(str(server), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> PlaygroundErrorBody | PlaygroundMcpError | PlaygroundMcpReply | None:
    if response.status_code == 200:
        response_200 = PlaygroundMcpReply.from_dict(response.json())

        return response_200

    if response.status_code == 400:
        response_400 = PlaygroundMcpError.from_dict(response.json())

        return response_400

    if response.status_code == 401:
        response_401 = PlaygroundErrorBody.from_dict(response.json())

        return response_401

    if response.status_code == 402:
        response_402 = PlaygroundMcpError.from_dict(response.json())

        return response_402

    if response.status_code == 403:
        response_403 = PlaygroundErrorBody.from_dict(response.json())

        return response_403

    if response.status_code == 404:
        response_404 = PlaygroundMcpError.from_dict(response.json())

        return response_404

    if response.status_code == 413:
        response_413 = PlaygroundMcpError.from_dict(response.json())

        return response_413

    if response.status_code == 429:
        response_429 = PlaygroundMcpError.from_dict(response.json())

        return response_429

    if response.status_code == 503:
        response_503 = PlaygroundMcpError.from_dict(response.json())

        return response_503

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[PlaygroundErrorBody | PlaygroundMcpError | PlaygroundMcpReply]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    server: str,
    *,
    client: AuthenticatedClient,
    body: PlaygroundMcpRequest,
    x_csrf_token: str | Unset = UNSET,
) -> Response[PlaygroundErrorBody | PlaygroundMcpError | PlaygroundMcpReply]:
    """
    Args:
        server (str):
        x_csrf_token (str | Unset):
        body (PlaygroundMcpRequest): A JSON-RPC request for `/mcp/{server}`, as the console's
            playground sends
            it: `tools/list` or `tools/call`. The body is read by the same dispatcher
            as that endpoint's.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[PlaygroundErrorBody | PlaygroundMcpError | PlaygroundMcpReply]
    """

    kwargs = _get_kwargs(
        server=server,
        body=body,
        x_csrf_token=x_csrf_token,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    server: str,
    *,
    client: AuthenticatedClient,
    body: PlaygroundMcpRequest,
    x_csrf_token: str | Unset = UNSET,
) -> PlaygroundErrorBody | PlaygroundMcpError | PlaygroundMcpReply | None:
    """
    Args:
        server (str):
        x_csrf_token (str | Unset):
        body (PlaygroundMcpRequest): A JSON-RPC request for `/mcp/{server}`, as the console's
            playground sends
            it: `tools/list` or `tools/call`. The body is read by the same dispatcher
            as that endpoint's.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        PlaygroundErrorBody | PlaygroundMcpError | PlaygroundMcpReply
    """

    return sync_detailed(
        server=server,
        client=client,
        body=body,
        x_csrf_token=x_csrf_token,
    ).parsed


async def asyncio_detailed(
    server: str,
    *,
    client: AuthenticatedClient,
    body: PlaygroundMcpRequest,
    x_csrf_token: str | Unset = UNSET,
) -> Response[PlaygroundErrorBody | PlaygroundMcpError | PlaygroundMcpReply]:
    """
    Args:
        server (str):
        x_csrf_token (str | Unset):
        body (PlaygroundMcpRequest): A JSON-RPC request for `/mcp/{server}`, as the console's
            playground sends
            it: `tools/list` or `tools/call`. The body is read by the same dispatcher
            as that endpoint's.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[PlaygroundErrorBody | PlaygroundMcpError | PlaygroundMcpReply]
    """

    kwargs = _get_kwargs(
        server=server,
        body=body,
        x_csrf_token=x_csrf_token,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    server: str,
    *,
    client: AuthenticatedClient,
    body: PlaygroundMcpRequest,
    x_csrf_token: str | Unset = UNSET,
) -> PlaygroundErrorBody | PlaygroundMcpError | PlaygroundMcpReply | None:
    """
    Args:
        server (str):
        x_csrf_token (str | Unset):
        body (PlaygroundMcpRequest): A JSON-RPC request for `/mcp/{server}`, as the console's
            playground sends
            it: `tools/list` or `tools/call`. The body is read by the same dispatcher
            as that endpoint's.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        PlaygroundErrorBody | PlaygroundMcpError | PlaygroundMcpReply
    """

    return (
        await asyncio_detailed(
            server=server,
            client=client,
            body=body,
            x_csrf_token=x_csrf_token,
        )
    ).parsed
