from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.grant_entry import GrantEntry
    from ..models.projection_entry import ProjectionEntry


T = TypeVar("T", bound="McpToolEntry")


@_attrs_define
class McpToolEntry:
    """What an admin set on a tool of an MCP server: whether it is on and who
    may call it. The tool itself (its schema) is the server's to say; it
    arrives with a sync.

        Attributes:
            enabled (bool):
            name (str):
            projection (None | ProjectionEntry):
            grants (GrantEntry | Unset):
    """

    enabled: bool
    name: str
    projection: None | ProjectionEntry
    grants: GrantEntry | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        from ..models.projection_entry import ProjectionEntry

        enabled = self.enabled

        name = self.name

        projection: dict[str, Any] | None
        if isinstance(self.projection, ProjectionEntry):
            projection = self.projection.to_dict()
        else:
            projection = self.projection

        grants: dict[str, Any] | Unset = UNSET
        if not isinstance(self.grants, Unset):
            grants = self.grants.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "enabled": enabled,
                "name": name,
                "projection": projection,
            }
        )
        if grants is not UNSET:
            field_dict["grants"] = grants

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.grant_entry import GrantEntry
        from ..models.projection_entry import ProjectionEntry

        d = dict(src_dict)
        enabled = d.pop("enabled")

        name = d.pop("name")

        def _parse_projection(data: object) -> None | ProjectionEntry:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                projection_type_0 = ProjectionEntry.from_dict(data)

                return projection_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | ProjectionEntry, data)

        projection = _parse_projection(d.pop("projection"))

        _grants = d.pop("grants", UNSET)
        grants: GrantEntry | Unset
        if isinstance(_grants, Unset):
            grants = UNSET
        else:
            grants = GrantEntry.from_dict(_grants)

        mcp_tool_entry = cls(
            enabled=enabled,
            name=name,
            projection=projection,
            grants=grants,
        )

        return mcp_tool_entry
