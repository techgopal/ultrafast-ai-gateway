from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.grant_entry import GrantEntry


T = TypeVar("T", bound="McpToolEntry")


@_attrs_define
class McpToolEntry:
    """What an admin set on a tool of an MCP server: whether it is on and who
    may call it. The tool itself (its schema) is the server's to say; it
    arrives with a sync.

        Attributes:
            enabled (bool):
            name (str):
            grants (GrantEntry | Unset):
    """

    enabled: bool
    name: str
    grants: GrantEntry | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        enabled = self.enabled

        name = self.name

        grants: dict[str, Any] | Unset = UNSET
        if not isinstance(self.grants, Unset):
            grants = self.grants.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "enabled": enabled,
                "name": name,
            }
        )
        if grants is not UNSET:
            field_dict["grants"] = grants

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.grant_entry import GrantEntry

        d = dict(src_dict)
        enabled = d.pop("enabled")

        name = d.pop("name")

        _grants = d.pop("grants", UNSET)
        grants: GrantEntry | Unset
        if isinstance(_grants, Unset):
            grants = UNSET
        else:
            grants = GrantEntry.from_dict(_grants)

        mcp_tool_entry = cls(
            enabled=enabled,
            name=name,
            grants=grants,
        )

        return mcp_tool_entry
