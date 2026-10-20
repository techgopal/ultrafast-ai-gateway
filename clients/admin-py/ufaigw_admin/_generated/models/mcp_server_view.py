from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.mcp_health_view import McpHealthView
    from ..models.mcp_local_view import McpLocalView


T = TypeVar("T", bound="McpServerView")


@_attrs_define
class McpServerView:
    """A server as `/api` shows it. Everything but the name and description is
    for admins: anybody else gets `null` (and a count of the tools they may
    call).

        Attributes:
            args (list[str] | None):
            auth_header (None | str):
            auth_kind (None | str): `none`, `bearer`, `header` or `oauth`.
            catalog_id (None | str): The catalog entry the server was made from.
            command (None | str): The program of a `stdio` server.
            created_at (str):
            description (str):
            enabled (bool):
            env_names (list[str] | None): The names of the environment variables of a `stdio` server; never the values.
            has_secret (bool | None):
            health (McpHealthView | None):
            id (int):
            last_sync_at (None | str):
            last_sync_error (None | str): Our summary of why the last sync failed; never text from the server.
            local (McpLocalView | None):
            name (str):
            oauth_client_id (None | str):
            oauth_grant (None | str):
            oauth_scopes (None | str):
            oauth_token_url (None | str): For `oauth`: the token endpoint, the client, the scopes and the grant.
            tool_count (int): How many tools: for an admin those the server offers, for anybody
                else those they may call.
            transport (str): `http`, or `stdio` for a program the gateway runs.
            url (None | str):
            usable (bool | None): The server can be called: it needs no secret, or its secret can be read.
    """

    args: list[str] | None
    auth_header: None | str
    auth_kind: None | str
    catalog_id: None | str
    command: None | str
    created_at: str
    description: str
    enabled: bool
    env_names: list[str] | None
    has_secret: bool | None
    health: McpHealthView | None
    id: int
    last_sync_at: None | str
    last_sync_error: None | str
    local: McpLocalView | None
    name: str
    oauth_client_id: None | str
    oauth_grant: None | str
    oauth_scopes: None | str
    oauth_token_url: None | str
    tool_count: int
    transport: str
    url: None | str
    usable: bool | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.mcp_health_view import McpHealthView
        from ..models.mcp_local_view import McpLocalView

        args: list[str] | None
        if isinstance(self.args, list):
            args = self.args

        else:
            args = self.args

        auth_header: None | str
        auth_header = self.auth_header

        auth_kind: None | str
        auth_kind = self.auth_kind

        catalog_id: None | str
        catalog_id = self.catalog_id

        command: None | str
        command = self.command

        created_at = self.created_at

        description = self.description

        enabled = self.enabled

        env_names: list[str] | None
        if isinstance(self.env_names, list):
            env_names = self.env_names

        else:
            env_names = self.env_names

        has_secret: bool | None
        has_secret = self.has_secret

        health: dict[str, Any] | None
        if isinstance(self.health, McpHealthView):
            health = self.health.to_dict()
        else:
            health = self.health

        id = self.id

        last_sync_at: None | str
        last_sync_at = self.last_sync_at

        last_sync_error: None | str
        last_sync_error = self.last_sync_error

        local: dict[str, Any] | None
        if isinstance(self.local, McpLocalView):
            local = self.local.to_dict()
        else:
            local = self.local

        name = self.name

        oauth_client_id: None | str
        oauth_client_id = self.oauth_client_id

        oauth_grant: None | str
        oauth_grant = self.oauth_grant

        oauth_scopes: None | str
        oauth_scopes = self.oauth_scopes

        oauth_token_url: None | str
        oauth_token_url = self.oauth_token_url

        tool_count = self.tool_count

        transport = self.transport

        url: None | str
        url = self.url

        usable: bool | None
        usable = self.usable

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "args": args,
                "auth_header": auth_header,
                "auth_kind": auth_kind,
                "catalog_id": catalog_id,
                "command": command,
                "created_at": created_at,
                "description": description,
                "enabled": enabled,
                "env_names": env_names,
                "has_secret": has_secret,
                "health": health,
                "id": id,
                "last_sync_at": last_sync_at,
                "last_sync_error": last_sync_error,
                "local": local,
                "name": name,
                "oauth_client_id": oauth_client_id,
                "oauth_grant": oauth_grant,
                "oauth_scopes": oauth_scopes,
                "oauth_token_url": oauth_token_url,
                "tool_count": tool_count,
                "transport": transport,
                "url": url,
                "usable": usable,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.mcp_health_view import McpHealthView
        from ..models.mcp_local_view import McpLocalView

        d = dict(src_dict)

        def _parse_args(data: object) -> list[str] | None:
            if data is None:
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                args_type_0 = cast(list[str], data)

                return args_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None, data)

        args = _parse_args(d.pop("args"))

        def _parse_auth_header(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        auth_header = _parse_auth_header(d.pop("auth_header"))

        def _parse_auth_kind(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        auth_kind = _parse_auth_kind(d.pop("auth_kind"))

        def _parse_catalog_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        catalog_id = _parse_catalog_id(d.pop("catalog_id"))

        def _parse_command(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        command = _parse_command(d.pop("command"))

        created_at = d.pop("created_at")

        description = d.pop("description")

        enabled = d.pop("enabled")

        def _parse_env_names(data: object) -> list[str] | None:
            if data is None:
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                env_names_type_0 = cast(list[str], data)

                return env_names_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None, data)

        env_names = _parse_env_names(d.pop("env_names"))

        def _parse_has_secret(data: object) -> bool | None:
            if data is None:
                return data
            return cast(bool | None, data)

        has_secret = _parse_has_secret(d.pop("has_secret"))

        def _parse_health(data: object) -> McpHealthView | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                health_type_0 = McpHealthView.from_dict(data)

                return health_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(McpHealthView | None, data)

        health = _parse_health(d.pop("health"))

        id = d.pop("id")

        def _parse_last_sync_at(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        last_sync_at = _parse_last_sync_at(d.pop("last_sync_at"))

        def _parse_last_sync_error(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        last_sync_error = _parse_last_sync_error(d.pop("last_sync_error"))

        def _parse_local(data: object) -> McpLocalView | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                local_type_0 = McpLocalView.from_dict(data)

                return local_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(McpLocalView | None, data)

        local = _parse_local(d.pop("local"))

        name = d.pop("name")

        def _parse_oauth_client_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        oauth_client_id = _parse_oauth_client_id(d.pop("oauth_client_id"))

        def _parse_oauth_grant(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        oauth_grant = _parse_oauth_grant(d.pop("oauth_grant"))

        def _parse_oauth_scopes(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        oauth_scopes = _parse_oauth_scopes(d.pop("oauth_scopes"))

        def _parse_oauth_token_url(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        oauth_token_url = _parse_oauth_token_url(d.pop("oauth_token_url"))

        tool_count = d.pop("tool_count")

        transport = d.pop("transport")

        def _parse_url(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        url = _parse_url(d.pop("url"))

        def _parse_usable(data: object) -> bool | None:
            if data is None:
                return data
            return cast(bool | None, data)

        usable = _parse_usable(d.pop("usable"))

        mcp_server_view = cls(
            args=args,
            auth_header=auth_header,
            auth_kind=auth_kind,
            catalog_id=catalog_id,
            command=command,
            created_at=created_at,
            description=description,
            enabled=enabled,
            env_names=env_names,
            has_secret=has_secret,
            health=health,
            id=id,
            last_sync_at=last_sync_at,
            last_sync_error=last_sync_error,
            local=local,
            name=name,
            oauth_client_id=oauth_client_id,
            oauth_grant=oauth_grant,
            oauth_scopes=oauth_scopes,
            oauth_token_url=oauth_token_url,
            tool_count=tool_count,
            transport=transport,
            url=url,
            usable=usable,
        )

        mcp_server_view.additional_properties = d
        return mcp_server_view

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
