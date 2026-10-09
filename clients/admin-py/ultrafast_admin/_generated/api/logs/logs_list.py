from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.api_error_body import ApiErrorBody
from ...models.log_page import LogPage
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    limit: int | Unset = UNSET,
    before: int | Unset = UNSET,
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    key_id: int | Unset = UNSET,
    user_id: int | Unset = UNSET,
    team_id: int | Unset = UNSET,
    model: str | Unset = UNSET,
    status: int | Unset = UNSET,
    errors: bool | Unset = UNSET,
    guardrail: str | Unset = UNSET,
    tag: list[str] | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["limit"] = limit

    params["before"] = before

    params["from"] = from_

    params["to"] = to

    params["key_id"] = key_id

    params["user_id"] = user_id

    params["team_id"] = team_id

    params["model"] = model

    params["status"] = status

    params["errors"] = errors

    params["guardrail"] = guardrail

    json_tag: list[str] | Unset = UNSET
    if not isinstance(tag, Unset):
        json_tag = tag

    params["tag"] = json_tag

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/logs",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ApiErrorBody | LogPage | None:
    if response.status_code == 200:
        response_200 = LogPage.from_dict(response.json())

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
) -> Response[ApiErrorBody | LogPage]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    limit: int | Unset = UNSET,
    before: int | Unset = UNSET,
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    key_id: int | Unset = UNSET,
    user_id: int | Unset = UNSET,
    team_id: int | Unset = UNSET,
    model: str | Unset = UNSET,
    status: int | Unset = UNSET,
    errors: bool | Unset = UNSET,
    guardrail: str | Unset = UNSET,
    tag: list[str] | Unset = UNSET,
) -> Response[ApiErrorBody | LogPage]:
    """Newest first. `before` is the id of the last row of the page before.
    Filters only narrow what the caller may see.

     What a caller sees depends on who they are. An admin sees every row. A
    team lead sees the rows of the teams they lead, the rows of the users who
    are members of those teams, and their own. Anyone else sees their own
    rows. A lead's scope uses the CURRENT membership: a row linked to a team
    only by its user leaves the lead's view when that user leaves the team.
    Rows of keys without an owner are visible to admins only.

    Args:
        limit (int | Unset):
        before (int | Unset):
        from_ (str | Unset):
        to (str | Unset):
        key_id (int | Unset):
        user_id (int | Unset):
        team_id (int | Unset):
        model (str | Unset):
        status (int | Unset):
        errors (bool | Unset):
        guardrail (str | Unset):
        tag (list[str] | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ApiErrorBody | LogPage]
    """

    kwargs = _get_kwargs(
        limit=limit,
        before=before,
        from_=from_,
        to=to,
        key_id=key_id,
        user_id=user_id,
        team_id=team_id,
        model=model,
        status=status,
        errors=errors,
        guardrail=guardrail,
        tag=tag,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
    limit: int | Unset = UNSET,
    before: int | Unset = UNSET,
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    key_id: int | Unset = UNSET,
    user_id: int | Unset = UNSET,
    team_id: int | Unset = UNSET,
    model: str | Unset = UNSET,
    status: int | Unset = UNSET,
    errors: bool | Unset = UNSET,
    guardrail: str | Unset = UNSET,
    tag: list[str] | Unset = UNSET,
) -> ApiErrorBody | LogPage | None:
    """Newest first. `before` is the id of the last row of the page before.
    Filters only narrow what the caller may see.

     What a caller sees depends on who they are. An admin sees every row. A
    team lead sees the rows of the teams they lead, the rows of the users who
    are members of those teams, and their own. Anyone else sees their own
    rows. A lead's scope uses the CURRENT membership: a row linked to a team
    only by its user leaves the lead's view when that user leaves the team.
    Rows of keys without an owner are visible to admins only.

    Args:
        limit (int | Unset):
        before (int | Unset):
        from_ (str | Unset):
        to (str | Unset):
        key_id (int | Unset):
        user_id (int | Unset):
        team_id (int | Unset):
        model (str | Unset):
        status (int | Unset):
        errors (bool | Unset):
        guardrail (str | Unset):
        tag (list[str] | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ApiErrorBody | LogPage
    """

    return sync_detailed(
        client=client,
        limit=limit,
        before=before,
        from_=from_,
        to=to,
        key_id=key_id,
        user_id=user_id,
        team_id=team_id,
        model=model,
        status=status,
        errors=errors,
        guardrail=guardrail,
        tag=tag,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    limit: int | Unset = UNSET,
    before: int | Unset = UNSET,
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    key_id: int | Unset = UNSET,
    user_id: int | Unset = UNSET,
    team_id: int | Unset = UNSET,
    model: str | Unset = UNSET,
    status: int | Unset = UNSET,
    errors: bool | Unset = UNSET,
    guardrail: str | Unset = UNSET,
    tag: list[str] | Unset = UNSET,
) -> Response[ApiErrorBody | LogPage]:
    """Newest first. `before` is the id of the last row of the page before.
    Filters only narrow what the caller may see.

     What a caller sees depends on who they are. An admin sees every row. A
    team lead sees the rows of the teams they lead, the rows of the users who
    are members of those teams, and their own. Anyone else sees their own
    rows. A lead's scope uses the CURRENT membership: a row linked to a team
    only by its user leaves the lead's view when that user leaves the team.
    Rows of keys without an owner are visible to admins only.

    Args:
        limit (int | Unset):
        before (int | Unset):
        from_ (str | Unset):
        to (str | Unset):
        key_id (int | Unset):
        user_id (int | Unset):
        team_id (int | Unset):
        model (str | Unset):
        status (int | Unset):
        errors (bool | Unset):
        guardrail (str | Unset):
        tag (list[str] | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ApiErrorBody | LogPage]
    """

    kwargs = _get_kwargs(
        limit=limit,
        before=before,
        from_=from_,
        to=to,
        key_id=key_id,
        user_id=user_id,
        team_id=team_id,
        model=model,
        status=status,
        errors=errors,
        guardrail=guardrail,
        tag=tag,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    limit: int | Unset = UNSET,
    before: int | Unset = UNSET,
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    key_id: int | Unset = UNSET,
    user_id: int | Unset = UNSET,
    team_id: int | Unset = UNSET,
    model: str | Unset = UNSET,
    status: int | Unset = UNSET,
    errors: bool | Unset = UNSET,
    guardrail: str | Unset = UNSET,
    tag: list[str] | Unset = UNSET,
) -> ApiErrorBody | LogPage | None:
    """Newest first. `before` is the id of the last row of the page before.
    Filters only narrow what the caller may see.

     What a caller sees depends on who they are. An admin sees every row. A
    team lead sees the rows of the teams they lead, the rows of the users who
    are members of those teams, and their own. Anyone else sees their own
    rows. A lead's scope uses the CURRENT membership: a row linked to a team
    only by its user leaves the lead's view when that user leaves the team.
    Rows of keys without an owner are visible to admins only.

    Args:
        limit (int | Unset):
        before (int | Unset):
        from_ (str | Unset):
        to (str | Unset):
        key_id (int | Unset):
        user_id (int | Unset):
        team_id (int | Unset):
        model (str | Unset):
        status (int | Unset):
        errors (bool | Unset):
        guardrail (str | Unset):
        tag (list[str] | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ApiErrorBody | LogPage
    """

    return (
        await asyncio_detailed(
            client=client,
            limit=limit,
            before=before,
            from_=from_,
            to=to,
            key_id=key_id,
            user_id=user_id,
            team_id=team_id,
            model=model,
            status=status,
            errors=errors,
            guardrail=guardrail,
            tag=tag,
        )
    ).parsed
