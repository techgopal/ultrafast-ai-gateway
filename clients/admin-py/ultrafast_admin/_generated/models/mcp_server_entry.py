from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.mcp_tool_entry import McpToolEntry


T = TypeVar("T", bound="McpServerEntry")


@_attrs_define
class McpServerEntry:
    """An MCP server. Its secret is never in a file: a server an import
    creates with authentication cannot be called until an admin enters it.

        Attributes:
            auth_header (None | str): The header name, for `auth_kind` `header`.
            name (str):
            url (str):
            auth_kind (str | Unset): `none`, `bearer` or `header`.
            description (str | Unset):
            enabled (bool | Unset):
            tools (list[McpToolEntry] | Unset): The tools the server offers that have settings. A tool the server
                does not offer yet is reported, not created.
    """

    auth_header: None | str
    name: str
    url: str
    auth_kind: str | Unset = UNSET
    description: str | Unset = UNSET
    enabled: bool | Unset = UNSET
    tools: list[McpToolEntry] | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        auth_header: None | str
        auth_header = self.auth_header

        name = self.name

        url = self.url

        auth_kind = self.auth_kind

        description = self.description

        enabled = self.enabled

        tools: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.tools, Unset):
            tools = []
            for tools_item_data in self.tools:
                tools_item = tools_item_data.to_dict()
                tools.append(tools_item)

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "auth_header": auth_header,
                "name": name,
                "url": url,
            }
        )
        if auth_kind is not UNSET:
            field_dict["auth_kind"] = auth_kind
        if description is not UNSET:
            field_dict["description"] = description
        if enabled is not UNSET:
            field_dict["enabled"] = enabled
        if tools is not UNSET:
            field_dict["tools"] = tools

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.mcp_tool_entry import McpToolEntry

        d = dict(src_dict)

        def _parse_auth_header(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        auth_header = _parse_auth_header(d.pop("auth_header"))

        name = d.pop("name")

        url = d.pop("url")

        auth_kind = d.pop("auth_kind", UNSET)

        description = d.pop("description", UNSET)

        enabled = d.pop("enabled", UNSET)

        _tools = d.pop("tools", UNSET)
        tools: list[McpToolEntry] | Unset = UNSET
        if _tools is not UNSET:
            tools = []
            for tools_item_data in _tools:
                tools_item = McpToolEntry.from_dict(tools_item_data)

                tools.append(tools_item)

        mcp_server_entry = cls(
            auth_header=auth_header,
            name=name,
            url=url,
            auth_kind=auth_kind,
            description=description,
            enabled=enabled,
            tools=tools,
        )

        return mcp_server_entry
