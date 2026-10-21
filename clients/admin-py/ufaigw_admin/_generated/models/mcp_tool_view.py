from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.grants_view import GrantsView
    from ..models.mcp_tool_view_annotations_type_0 import McpToolViewAnnotationsType0
    from ..models.mcp_tool_view_input_schema import McpToolViewInputSchema
    from ..models.mcp_tool_view_output_schema_type_0 import McpToolViewOutputSchemaType0
    from ..models.projection_body import ProjectionBody


T = TypeVar("T", bound="McpToolView")


@_attrs_define
class McpToolView:
    """
    Attributes:
        annotations (McpToolViewAnnotationsType0 | None):
        description (str):
        enabled (bool):
        grants (GrantsView): Who may call a model. For everyone who is not an admin it is always
            empty.
        id (int):
        input_schema (McpToolViewInputSchema): JSON Schema of the arguments, as the server gave it.
        name (str):
        output_schema (McpToolViewOutputSchemaType0 | None):
        projection (None | ProjectionBody):
        removed (bool): The server no longer offers it; its grants are kept.
        server_id (int):
        title (None | str):
    """

    annotations: McpToolViewAnnotationsType0 | None
    description: str
    enabled: bool
    grants: GrantsView
    id: int
    input_schema: McpToolViewInputSchema
    name: str
    output_schema: McpToolViewOutputSchemaType0 | None
    projection: None | ProjectionBody
    removed: bool
    server_id: int
    title: None | str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.mcp_tool_view_annotations_type_0 import (
            McpToolViewAnnotationsType0,
        )
        from ..models.mcp_tool_view_output_schema_type_0 import (
            McpToolViewOutputSchemaType0,
        )
        from ..models.projection_body import ProjectionBody

        annotations: dict[str, Any] | None
        if isinstance(self.annotations, McpToolViewAnnotationsType0):
            annotations = self.annotations.to_dict()
        else:
            annotations = self.annotations

        description = self.description

        enabled = self.enabled

        grants = self.grants.to_dict()

        id = self.id

        input_schema = self.input_schema.to_dict()

        name = self.name

        output_schema: dict[str, Any] | None
        if isinstance(self.output_schema, McpToolViewOutputSchemaType0):
            output_schema = self.output_schema.to_dict()
        else:
            output_schema = self.output_schema

        projection: dict[str, Any] | None
        if isinstance(self.projection, ProjectionBody):
            projection = self.projection.to_dict()
        else:
            projection = self.projection

        removed = self.removed

        server_id = self.server_id

        title: None | str
        title = self.title

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "annotations": annotations,
                "description": description,
                "enabled": enabled,
                "grants": grants,
                "id": id,
                "input_schema": input_schema,
                "name": name,
                "output_schema": output_schema,
                "projection": projection,
                "removed": removed,
                "server_id": server_id,
                "title": title,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.grants_view import GrantsView
        from ..models.mcp_tool_view_annotations_type_0 import (
            McpToolViewAnnotationsType0,
        )
        from ..models.mcp_tool_view_input_schema import (
            McpToolViewInputSchema,
        )
        from ..models.mcp_tool_view_output_schema_type_0 import (
            McpToolViewOutputSchemaType0,
        )
        from ..models.projection_body import ProjectionBody

        d = dict(src_dict)

        def _parse_annotations(data: object) -> McpToolViewAnnotationsType0 | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                annotations_type_0 = McpToolViewAnnotationsType0.from_dict(data)

                return annotations_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(McpToolViewAnnotationsType0 | None, data)

        annotations = _parse_annotations(d.pop("annotations"))

        description = d.pop("description")

        enabled = d.pop("enabled")

        grants = GrantsView.from_dict(d.pop("grants"))

        id = d.pop("id")

        input_schema = McpToolViewInputSchema.from_dict(d.pop("input_schema"))

        name = d.pop("name")

        def _parse_output_schema(data: object) -> McpToolViewOutputSchemaType0 | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                output_schema_type_0 = McpToolViewOutputSchemaType0.from_dict(data)

                return output_schema_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(McpToolViewOutputSchemaType0 | None, data)

        output_schema = _parse_output_schema(d.pop("output_schema"))

        def _parse_projection(data: object) -> None | ProjectionBody:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                projection_type_0 = ProjectionBody.from_dict(data)

                return projection_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | ProjectionBody, data)

        projection = _parse_projection(d.pop("projection"))

        removed = d.pop("removed")

        server_id = d.pop("server_id")

        def _parse_title(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        title = _parse_title(d.pop("title"))

        mcp_tool_view = cls(
            annotations=annotations,
            description=description,
            enabled=enabled,
            grants=grants,
            id=id,
            input_schema=input_schema,
            name=name,
            output_schema=output_schema,
            projection=projection,
            removed=removed,
            server_id=server_id,
            title=title,
        )

        mcp_tool_view.additional_properties = d
        return mcp_tool_view

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
