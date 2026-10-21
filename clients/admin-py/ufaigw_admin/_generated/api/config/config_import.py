from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error_body import ApiErrorBody
from ...models.config_file import ConfigFile
from ...models.import_report import ImportReport
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: ConfigFile,
    dry_run: bool | Unset = UNSET,
    x_csrf_token: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_csrf_token, Unset):
        headers["x-csrf-token"] = x_csrf_token

    params: dict[str, Any] = {}

    params["dry_run"] = dry_run

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/api/config/import",
        "params": params,
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ApiErrorBody | ImportReport | None:
    if response.status_code == 200:
        response_200 = ImportReport.from_dict(response.json())

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

    if response.status_code == 413:
        response_413 = ApiErrorBody.from_dict(response.json())

        return response_413

    if response.status_code == 422:
        response_422 = ImportReport.from_dict(response.json())

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
) -> Response[ApiErrorBody | ImportReport]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: ConfigFile,
    dry_run: bool | Unset = UNSET,
    x_csrf_token: str | Unset = UNSET,
) -> Response[ApiErrorBody | ImportReport]:
    """Checks a configuration file and, with `dry_run=false`, writes it. Without
    `dry_run` it is a dry run. Missing things are created and existing ones,
    by name, are updated; nothing is deleted. A file with any error writes
    nothing: the answer is 422 with the report. A new provider has no
    credential until an admin sets one.

    Args:
        dry_run (bool | Unset):
        x_csrf_token (str | Unset):
        body (ConfigFile): The configuration file. `format` and `version` come first.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ApiErrorBody | ImportReport]
    """

    kwargs = _get_kwargs(
        body=body,
        dry_run=dry_run,
        x_csrf_token=x_csrf_token,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
    body: ConfigFile,
    dry_run: bool | Unset = UNSET,
    x_csrf_token: str | Unset = UNSET,
) -> ApiErrorBody | ImportReport | None:
    """Checks a configuration file and, with `dry_run=false`, writes it. Without
    `dry_run` it is a dry run. Missing things are created and existing ones,
    by name, are updated; nothing is deleted. A file with any error writes
    nothing: the answer is 422 with the report. A new provider has no
    credential until an admin sets one.

    Args:
        dry_run (bool | Unset):
        x_csrf_token (str | Unset):
        body (ConfigFile): The configuration file. `format` and `version` come first.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ApiErrorBody | ImportReport
    """

    return sync_detailed(
        client=client,
        body=body,
        dry_run=dry_run,
        x_csrf_token=x_csrf_token,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: ConfigFile,
    dry_run: bool | Unset = UNSET,
    x_csrf_token: str | Unset = UNSET,
) -> Response[ApiErrorBody | ImportReport]:
    """Checks a configuration file and, with `dry_run=false`, writes it. Without
    `dry_run` it is a dry run. Missing things are created and existing ones,
    by name, are updated; nothing is deleted. A file with any error writes
    nothing: the answer is 422 with the report. A new provider has no
    credential until an admin sets one.

    Args:
        dry_run (bool | Unset):
        x_csrf_token (str | Unset):
        body (ConfigFile): The configuration file. `format` and `version` come first.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ApiErrorBody | ImportReport]
    """

    kwargs = _get_kwargs(
        body=body,
        dry_run=dry_run,
        x_csrf_token=x_csrf_token,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: ConfigFile,
    dry_run: bool | Unset = UNSET,
    x_csrf_token: str | Unset = UNSET,
) -> ApiErrorBody | ImportReport | None:
    """Checks a configuration file and, with `dry_run=false`, writes it. Without
    `dry_run` it is a dry run. Missing things are created and existing ones,
    by name, are updated; nothing is deleted. A file with any error writes
    nothing: the answer is 422 with the report. A new provider has no
    credential until an admin sets one.

    Args:
        dry_run (bool | Unset):
        x_csrf_token (str | Unset):
        body (ConfigFile): The configuration file. `format` and `version` come first.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ApiErrorBody | ImportReport
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            dry_run=dry_run,
            x_csrf_token=x_csrf_token,
        )
    ).parsed
