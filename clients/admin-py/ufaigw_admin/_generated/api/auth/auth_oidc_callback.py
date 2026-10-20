from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error_body import ApiErrorBody
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    code: str | Unset = UNSET,
    state: str | Unset = UNSET,
    error: str | Unset = UNSET,
    error_description: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["code"] = code

    params["state"] = state

    params["error"] = error

    params["error_description"] = error_description

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/auth/oidc/callback",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | ApiErrorBody | None:
    if response.status_code == 302:
        response_302 = cast(Any, None)
        return response_302

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
    code: str | Unset = UNSET,
    state: str | Unset = UNSET,
    error: str | Unset = UNSET,
    error_description: str | Unset = UNSET,
) -> Response[Any | ApiErrorBody]:
    """Finish signing in with the identity provider

     Where the identity provider sends the browser back: a GET that needs no session and no CSRF header,
    the flow cookie and `state` being what ties it to the start. Limited to 60 callbacks per client
    address in 15 minutes, counted apart from password sign-in failures (a callback can never lock
    anyone out of password sign-in); without a provider nothing is counted and the answer is `config`.
    Over the limit the browser is sent to `/sign-in?sso_error=rate_limited`.

    Args:
        code (str | Unset):
        state (str | Unset):
        error (str | Unset):
        error_description (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorBody]
    """

    kwargs = _get_kwargs(
        code=code,
        state=state,
        error=error,
        error_description=error_description,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    code: str | Unset = UNSET,
    state: str | Unset = UNSET,
    error: str | Unset = UNSET,
    error_description: str | Unset = UNSET,
) -> Any | ApiErrorBody | None:
    """Finish signing in with the identity provider

     Where the identity provider sends the browser back: a GET that needs no session and no CSRF header,
    the flow cookie and `state` being what ties it to the start. Limited to 60 callbacks per client
    address in 15 minutes, counted apart from password sign-in failures (a callback can never lock
    anyone out of password sign-in); without a provider nothing is counted and the answer is `config`.
    Over the limit the browser is sent to `/sign-in?sso_error=rate_limited`.

    Args:
        code (str | Unset):
        state (str | Unset):
        error (str | Unset):
        error_description (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorBody
    """

    return sync_detailed(
        client=client,
        code=code,
        state=state,
        error=error,
        error_description=error_description,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    code: str | Unset = UNSET,
    state: str | Unset = UNSET,
    error: str | Unset = UNSET,
    error_description: str | Unset = UNSET,
) -> Response[Any | ApiErrorBody]:
    """Finish signing in with the identity provider

     Where the identity provider sends the browser back: a GET that needs no session and no CSRF header,
    the flow cookie and `state` being what ties it to the start. Limited to 60 callbacks per client
    address in 15 minutes, counted apart from password sign-in failures (a callback can never lock
    anyone out of password sign-in); without a provider nothing is counted and the answer is `config`.
    Over the limit the browser is sent to `/sign-in?sso_error=rate_limited`.

    Args:
        code (str | Unset):
        state (str | Unset):
        error (str | Unset):
        error_description (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorBody]
    """

    kwargs = _get_kwargs(
        code=code,
        state=state,
        error=error,
        error_description=error_description,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    code: str | Unset = UNSET,
    state: str | Unset = UNSET,
    error: str | Unset = UNSET,
    error_description: str | Unset = UNSET,
) -> Any | ApiErrorBody | None:
    """Finish signing in with the identity provider

     Where the identity provider sends the browser back: a GET that needs no session and no CSRF header,
    the flow cookie and `state` being what ties it to the start. Limited to 60 callbacks per client
    address in 15 minutes, counted apart from password sign-in failures (a callback can never lock
    anyone out of password sign-in); without a provider nothing is counted and the answer is `config`.
    Over the limit the browser is sent to `/sign-in?sso_error=rate_limited`.

    Args:
        code (str | Unset):
        state (str | Unset):
        error (str | Unset):
        error_description (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorBody
    """

    return (
        await asyncio_detailed(
            client=client,
            code=code,
            state=state,
            error=error,
            error_description=error_description,
        )
    ).parsed
