from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="McpServerView")


@_attrs_define
class McpServerView:
    """A server as `/api` shows it. Everything but the name and description is
    for admins: anybody else gets `null` (and a count of the tools they may
    call).

        Attributes:
            auth_header (None | str):
            auth_kind (None | str): `none`, `bearer` or `header`.
            created_at (str):
            description (str):
            enabled (bool):
            has_secret (bool | None):
            id (int):
            last_sync_at (None | str):
            last_sync_error (None | str): Our summary of why the last sync failed; never text from the server.
            name (str):
            tool_count (int): How many tools: for an admin those the server offers, for anybody
                else those they may call.
            url (None | str):
            usable (bool | None): The server can be called: it needs no secret, or its secret can be read.
    """

    auth_header: None | str
    auth_kind: None | str
    created_at: str
    description: str
    enabled: bool
    has_secret: bool | None
    id: int
    last_sync_at: None | str
    last_sync_error: None | str
    name: str
    tool_count: int
    url: None | str
    usable: bool | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        auth_header: None | str
        auth_header = self.auth_header

        auth_kind: None | str
        auth_kind = self.auth_kind

        created_at = self.created_at

        description = self.description

        enabled = self.enabled

        has_secret: bool | None
        has_secret = self.has_secret

        id = self.id

        last_sync_at: None | str
        last_sync_at = self.last_sync_at

        last_sync_error: None | str
        last_sync_error = self.last_sync_error

        name = self.name

        tool_count = self.tool_count

        url: None | str
        url = self.url

        usable: bool | None
        usable = self.usable

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "auth_header": auth_header,
                "auth_kind": auth_kind,
                "created_at": created_at,
                "description": description,
                "enabled": enabled,
                "has_secret": has_secret,
                "id": id,
                "last_sync_at": last_sync_at,
                "last_sync_error": last_sync_error,
                "name": name,
                "tool_count": tool_count,
                "url": url,
                "usable": usable,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

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

        created_at = d.pop("created_at")

        description = d.pop("description")

        enabled = d.pop("enabled")

        def _parse_has_secret(data: object) -> bool | None:
            if data is None:
                return data
            return cast(bool | None, data)

        has_secret = _parse_has_secret(d.pop("has_secret"))

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

        name = d.pop("name")

        tool_count = d.pop("tool_count")

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
            auth_header=auth_header,
            auth_kind=auth_kind,
            created_at=created_at,
            description=description,
            enabled=enabled,
            has_secret=has_secret,
            id=id,
            last_sync_at=last_sync_at,
            last_sync_error=last_sync_error,
            name=name,
            tool_count=tool_count,
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
