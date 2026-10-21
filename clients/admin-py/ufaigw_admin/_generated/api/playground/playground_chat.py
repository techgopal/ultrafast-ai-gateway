from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.playground_chat_answer import PlaygroundChatAnswer
from ...models.playground_chat_request import PlaygroundChatRequest
from ...models.playground_error_body import PlaygroundErrorBody
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: PlaygroundChatRequest,
    x_csrf_token: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_csrf_token, Unset):
        headers["x-csrf-token"] = x_csrf_token

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/api/playground/chat",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> PlaygroundChatAnswer | PlaygroundErrorBody | None:
    if response.status_code == 200:
        response_200 = PlaygroundChatAnswer.from_dict(response.json())

        return response_200

    if response.status_code == 400:
        response_400 = PlaygroundErrorBody.from_dict(response.json())

        return response_400

    if response.status_code == 401:
        response_401 = PlaygroundErrorBody.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = PlaygroundErrorBody.from_dict(response.json())

        return response_403

    if response.status_code == 404:
        response_404 = PlaygroundErrorBody.from_dict(response.json())

        return response_404

    if response.status_code == 429:
        response_429 = PlaygroundErrorBody.from_dict(response.json())

        return response_429

    if response.status_code == 502:
        response_502 = PlaygroundErrorBody.from_dict(response.json())

        return response_502

    if response.status_code == 503:
        response_503 = PlaygroundErrorBody.from_dict(response.json())

        return response_503

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[PlaygroundChatAnswer | PlaygroundErrorBody]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: PlaygroundChatRequest,
    x_csrf_token: str | Unset = UNSET,
) -> Response[PlaygroundChatAnswer | PlaygroundErrorBody]:
    """
    Args:
        x_csrf_token (str | Unset):
        body (PlaygroundChatRequest): A chat request, as `/v1/chat/completions` takes it. The body
            is read by
            the same parser as that call's, so any field it accepts is accepted here.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[PlaygroundChatAnswer | PlaygroundErrorBody]
    """

    kwargs = _get_kwargs(
        body=body,
        x_csrf_token=x_csrf_token,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
    body: PlaygroundChatRequest,
    x_csrf_token: str | Unset = UNSET,
) -> PlaygroundChatAnswer | PlaygroundErrorBody | None:
    """
    Args:
        x_csrf_token (str | Unset):
        body (PlaygroundChatRequest): A chat request, as `/v1/chat/completions` takes it. The body
            is read by
            the same parser as that call's, so any field it accepts is accepted here.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        PlaygroundChatAnswer | PlaygroundErrorBody
    """

    return sync_detailed(
        client=client,
        body=body,
        x_csrf_token=x_csrf_token,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: PlaygroundChatRequest,
    x_csrf_token: str | Unset = UNSET,
) -> Response[PlaygroundChatAnswer | PlaygroundErrorBody]:
    """
    Args:
        x_csrf_token (str | Unset):
        body (PlaygroundChatRequest): A chat request, as `/v1/chat/completions` takes it. The body
            is read by
            the same parser as that call's, so any field it accepts is accepted here.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[PlaygroundChatAnswer | PlaygroundErrorBody]
    """

    kwargs = _get_kwargs(
        body=body,
        x_csrf_token=x_csrf_token,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: PlaygroundChatRequest,
    x_csrf_token: str | Unset = UNSET,
) -> PlaygroundChatAnswer | PlaygroundErrorBody | None:
    """
    Args:
        x_csrf_token (str | Unset):
        body (PlaygroundChatRequest): A chat request, as `/v1/chat/completions` takes it. The body
            is read by
            the same parser as that call's, so any field it accepts is accepted here.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        PlaygroundChatAnswer | PlaygroundErrorBody
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            x_csrf_token=x_csrf_token,
        )
    ).parsed
