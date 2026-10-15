from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.create_mcp_server_request_env_type_0 import (
        CreateMcpServerRequestEnvType0,
    )


T = TypeVar("T", bound="CreateMcpServerRequest")


@_attrs_define
class CreateMcpServerRequest:
    """
    Attributes:
        name (str): 1 to 63 characters of `a-z`, `0-9` and `-`, starting with a letter or
            a digit. It is the name in the endpoint `/mcp/{name}` and in a key's
            allowlist (`mcp:{name}/{tool}`), and cannot be changed.
        args (list[str] | None | Unset):
        auth_header (None | str | Unset): The header that carries the secret, for `auth_kind` `header`.
        auth_kind (None | str | Unset): `none` (the default), `bearer`, `header` or `oauth`. Only for `http`.
        command (None | str | Unset): The program of a `stdio` server. It is run as it is, with `args`:
            there is no shell.
        description (None | str | Unset):
        enabled (bool | None | Unset): On by default.
        env (CreateMcpServerRequestEnvType0 | None | Unset): The environment of a `stdio` server, name to value; nothing
            else of
            the gateway's environment reaches the program (it also gets `PATH`,
            `HOME` and `LANG`). The values are stored encrypted and never shown.
        oauth_client_id (None | str | Unset): For `oauth`: the client the gateway is at the token endpoint.
        oauth_grant (None | str | Unset): For `oauth`: `client_credentials` (the default) or `token_exchange`
            (RFC 8693, with `secret` as the token to exchange). The token is made
            for this server's URL (RFC 8707). The gateway never passes an agent's
            key on, nor a token to an agent.
        oauth_scopes (None | str | Unset): For `oauth`: the scopes to ask for, space separated.
        oauth_token_url (None | str | Unset): For `auth_kind` `oauth`: where the gateway gets a token for this
            server. HTTPS (unless private addresses are allowed), on an
            authorization server the MCP server names, if it names any.
        secret (None | str | Unset): Required with `bearer`, `header` or `oauth` (the client secret, or for
            `token_exchange` the token to exchange).
        transport (None | str | Unset): `http` (the default), or `stdio` for a program the gateway runs on its
            own host. Local servers need `UF_MCP_LOCAL=on`.
        url (None | str | Unset): The streamable-HTTP endpoint of the server. Required for `http`,
            absent for `stdio`.
    """

    name: str
    args: list[str] | None | Unset = UNSET
    auth_header: None | str | Unset = UNSET
    auth_kind: None | str | Unset = UNSET
    command: None | str | Unset = UNSET
    description: None | str | Unset = UNSET
    enabled: bool | None | Unset = UNSET
    env: CreateMcpServerRequestEnvType0 | None | Unset = UNSET
    oauth_client_id: None | str | Unset = UNSET
    oauth_grant: None | str | Unset = UNSET
    oauth_scopes: None | str | Unset = UNSET
    oauth_token_url: None | str | Unset = UNSET
    secret: None | str | Unset = UNSET
    transport: None | str | Unset = UNSET
    url: None | str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        from ..models.create_mcp_server_request_env_type_0 import (
            CreateMcpServerRequestEnvType0,
        )

        name = self.name

        args: list[str] | None | Unset
        if isinstance(self.args, Unset):
            args = UNSET
        elif isinstance(self.args, list):
            args = self.args

        else:
            args = self.args

        auth_header: None | str | Unset
        if isinstance(self.auth_header, Unset):
            auth_header = UNSET
        else:
            auth_header = self.auth_header

        auth_kind: None | str | Unset
        if isinstance(self.auth_kind, Unset):
            auth_kind = UNSET
        else:
            auth_kind = self.auth_kind

        command: None | str | Unset
        if isinstance(self.command, Unset):
            command = UNSET
        else:
            command = self.command

        description: None | str | Unset
        if isinstance(self.description, Unset):
            description = UNSET
        else:
            description = self.description

        enabled: bool | None | Unset
        if isinstance(self.enabled, Unset):
            enabled = UNSET
        else:
            enabled = self.enabled

        env: dict[str, Any] | None | Unset
        if isinstance(self.env, Unset):
            env = UNSET
        elif isinstance(self.env, CreateMcpServerRequestEnvType0):
            env = self.env.to_dict()
        else:
            env = self.env

        oauth_client_id: None | str | Unset
        if isinstance(self.oauth_client_id, Unset):
            oauth_client_id = UNSET
        else:
            oauth_client_id = self.oauth_client_id

        oauth_grant: None | str | Unset
        if isinstance(self.oauth_grant, Unset):
            oauth_grant = UNSET
        else:
            oauth_grant = self.oauth_grant

        oauth_scopes: None | str | Unset
        if isinstance(self.oauth_scopes, Unset):
            oauth_scopes = UNSET
        else:
            oauth_scopes = self.oauth_scopes

        oauth_token_url: None | str | Unset
        if isinstance(self.oauth_token_url, Unset):
            oauth_token_url = UNSET
        else:
            oauth_token_url = self.oauth_token_url

        secret: None | str | Unset
        if isinstance(self.secret, Unset):
            secret = UNSET
        else:
            secret = self.secret

        transport: None | str | Unset
        if isinstance(self.transport, Unset):
            transport = UNSET
        else:
            transport = self.transport

        url: None | str | Unset
        if isinstance(self.url, Unset):
            url = UNSET
        else:
            url = self.url

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "name": name,
            }
        )
        if args is not UNSET:
            field_dict["args"] = args
        if auth_header is not UNSET:
            field_dict["auth_header"] = auth_header
        if auth_kind is not UNSET:
            field_dict["auth_kind"] = auth_kind
        if command is not UNSET:
            field_dict["command"] = command
        if description is not UNSET:
            field_dict["description"] = description
        if enabled is not UNSET:
            field_dict["enabled"] = enabled
        if env is not UNSET:
            field_dict["env"] = env
        if oauth_client_id is not UNSET:
            field_dict["oauth_client_id"] = oauth_client_id
        if oauth_grant is not UNSET:
            field_dict["oauth_grant"] = oauth_grant
        if oauth_scopes is not UNSET:
            field_dict["oauth_scopes"] = oauth_scopes
        if oauth_token_url is not UNSET:
            field_dict["oauth_token_url"] = oauth_token_url
        if secret is not UNSET:
            field_dict["secret"] = secret
        if transport is not UNSET:
            field_dict["transport"] = transport
        if url is not UNSET:
            field_dict["url"] = url

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.create_mcp_server_request_env_type_0 import (
            CreateMcpServerRequestEnvType0,
        )

        d = dict(src_dict)
        name = d.pop("name")

        def _parse_args(data: object) -> list[str] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                args_type_0 = cast(list[str], data)

                return args_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None | Unset, data)

        args = _parse_args(d.pop("args", UNSET))

        def _parse_auth_header(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        auth_header = _parse_auth_header(d.pop("auth_header", UNSET))

        def _parse_auth_kind(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        auth_kind = _parse_auth_kind(d.pop("auth_kind", UNSET))

        def _parse_command(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        command = _parse_command(d.pop("command", UNSET))

        def _parse_description(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        description = _parse_description(d.pop("description", UNSET))

        def _parse_enabled(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        enabled = _parse_enabled(d.pop("enabled", UNSET))

        def _parse_env(data: object) -> CreateMcpServerRequestEnvType0 | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                env_type_0 = CreateMcpServerRequestEnvType0.from_dict(data)

                return env_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(CreateMcpServerRequestEnvType0 | None | Unset, data)

        env = _parse_env(d.pop("env", UNSET))

        def _parse_oauth_client_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        oauth_client_id = _parse_oauth_client_id(d.pop("oauth_client_id", UNSET))

        def _parse_oauth_grant(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        oauth_grant = _parse_oauth_grant(d.pop("oauth_grant", UNSET))

        def _parse_oauth_scopes(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        oauth_scopes = _parse_oauth_scopes(d.pop("oauth_scopes", UNSET))

        def _parse_oauth_token_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        oauth_token_url = _parse_oauth_token_url(d.pop("oauth_token_url", UNSET))

        def _parse_secret(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        secret = _parse_secret(d.pop("secret", UNSET))

        def _parse_transport(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        transport = _parse_transport(d.pop("transport", UNSET))

        def _parse_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        url = _parse_url(d.pop("url", UNSET))

        create_mcp_server_request = cls(
            name=name,
            args=args,
            auth_header=auth_header,
            auth_kind=auth_kind,
            command=command,
            description=description,
            enabled=enabled,
            env=env,
            oauth_client_id=oauth_client_id,
            oauth_grant=oauth_grant,
            oauth_scopes=oauth_scopes,
            oauth_token_url=oauth_token_url,
            secret=secret,
            transport=transport,
            url=url,
        )

        return create_mcp_server_request
