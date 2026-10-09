from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error_body import ApiErrorBody
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    return_to: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["return_to"] = return_to

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/auth/oidc/start",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | ApiErrorBody | None:
    if response.status_code == 302:
        response_302 = cast(Any, None)
        return response_302

    if response.status_code == 404:
        response_404 = ApiErrorBody.from_dict(response.json())

        return response_404

    if response.status_code == 500:
        response_500 = ApiErrorBody.from_dict(response.json())

        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Any | ApiErrorBody]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    return_to: str | Unset = UNSET,
) -> Response[Any | ApiErrorBody]:
    """Start signing in with the identity provider

     A browser navigation, not a call for a script: a GET that needs no session and no CSRF header.
    Limited to 60 starts per client address in 15 minutes, counted apart from sign-in failures; over the
    limit the browser is sent to `/sign-in?sso_error=rate_limited` and a flow cookie already set is left
    alone.

    Args:
        return_to (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorBody]
    """

    kwargs = _get_kwargs(
        return_to=return_to,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    return_to: str | Unset = UNSET,
) -> Any | ApiErrorBody | None:
    """Start signing in with the identity provider

     A browser navigation, not a call for a script: a GET that needs no session and no CSRF header.
    Limited to 60 starts per client address in 15 minutes, counted apart from sign-in failures; over the
    limit the browser is sent to `/sign-in?sso_error=rate_limited` and a flow cookie already set is left
    alone.

    Args:
        return_to (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorBody
    """

    return sync_detailed(
        client=client,
        return_to=return_to,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    return_to: str | Unset = UNSET,
) -> Response[Any | ApiErrorBody]:
    """Start signing in with the identity provider

     A browser navigation, not a call for a script: a GET that needs no session and no CSRF header.
    Limited to 60 starts per client address in 15 minutes, counted apart from sign-in failures; over the
    limit the browser is sent to `/sign-in?sso_error=rate_limited` and a flow cookie already set is left
    alone.

    Args:
        return_to (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorBody]
    """

    kwargs = _get_kwargs(
        return_to=return_to,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    return_to: str | Unset = UNSET,
) -> Any | ApiErrorBody | None:
    """Start signing in with the identity provider

     A browser navigation, not a call for a script: a GET that needs no session and no CSRF header.
    Limited to 60 starts per client address in 15 minutes, counted apart from sign-in failures; over the
    limit the browser is sent to `/sign-in?sso_error=rate_limited` and a flow cookie already set is left
    alone.

    Args:
        return_to (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorBody
    """

    return (
        await asyncio_detailed(
            client=client,
            return_to=return_to,
        )
    ).parsed
