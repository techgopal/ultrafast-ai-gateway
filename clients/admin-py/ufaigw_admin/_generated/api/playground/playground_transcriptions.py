from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.playground_error_body import PlaygroundErrorBody
from ...models.playground_transcription_form import PlaygroundTranscriptionForm
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: PlaygroundTranscriptionForm,
    x_csrf_token: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_csrf_token, Unset):
        headers["x-csrf-token"] = x_csrf_token

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/api/playground/transcriptions",
    }

    _kwargs["files"] = body.to_multipart()

    headers["Content-Type"] = "multipart/form-data; boundary=+++"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> PlaygroundErrorBody | str | None:
    if response.status_code == 200:
        response_200 = cast(str, response.json())
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

    if response.status_code == 413:
        response_413 = PlaygroundErrorBody.from_dict(response.json())

        return response_413

    if response.status_code == 429:
        response_429 = PlaygroundErrorBody.from_dict(response.json())

        return response_429

    if response.status_code == 502:
        response_502 = PlaygroundErrorBody.from_dict(response.json())

        return response_502

    if response.status_code == 503:
        response_503 = PlaygroundErrorBody.from_dict(response.json())

        return response_503

    if response.status_code == 504:
        response_504 = PlaygroundErrorBody.from_dict(response.json())

        return response_504

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[PlaygroundErrorBody | str]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: PlaygroundTranscriptionForm,
    x_csrf_token: str | Unset = UNSET,
) -> Response[PlaygroundErrorBody | str]:
    """
    Args:
        x_csrf_token (str | Unset):
        body (PlaygroundTranscriptionForm): The form of a transcription, as
            `/v1/audio/transcriptions` takes it
            (`multipart/form-data`). The form is read by the same reader as that
            call's: the file may not be larger than the gateway's audio cap
            (`UF_MAX_AUDIO_BYTES`, 25 MiB by default).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[PlaygroundErrorBody | str]
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
    body: PlaygroundTranscriptionForm,
    x_csrf_token: str | Unset = UNSET,
) -> PlaygroundErrorBody | str | None:
    """
    Args:
        x_csrf_token (str | Unset):
        body (PlaygroundTranscriptionForm): The form of a transcription, as
            `/v1/audio/transcriptions` takes it
            (`multipart/form-data`). The form is read by the same reader as that
            call's: the file may not be larger than the gateway's audio cap
            (`UF_MAX_AUDIO_BYTES`, 25 MiB by default).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        PlaygroundErrorBody | str
    """

    return sync_detailed(
        client=client,
        body=body,
        x_csrf_token=x_csrf_token,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: PlaygroundTranscriptionForm,
    x_csrf_token: str | Unset = UNSET,
) -> Response[PlaygroundErrorBody | str]:
    """
    Args:
        x_csrf_token (str | Unset):
        body (PlaygroundTranscriptionForm): The form of a transcription, as
            `/v1/audio/transcriptions` takes it
            (`multipart/form-data`). The form is read by the same reader as that
            call's: the file may not be larger than the gateway's audio cap
            (`UF_MAX_AUDIO_BYTES`, 25 MiB by default).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[PlaygroundErrorBody | str]
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
    body: PlaygroundTranscriptionForm,
    x_csrf_token: str | Unset = UNSET,
) -> PlaygroundErrorBody | str | None:
    """
    Args:
        x_csrf_token (str | Unset):
        body (PlaygroundTranscriptionForm): The form of a transcription, as
            `/v1/audio/transcriptions` takes it
            (`multipart/form-data`). The form is read by the same reader as that
            call's: the file may not be larger than the gateway's audio cap
            (`UF_MAX_AUDIO_BYTES`, 25 MiB by default).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        PlaygroundErrorBody | str
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            x_csrf_token=x_csrf_token,
        )
    ).parsed
