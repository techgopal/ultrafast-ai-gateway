from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.projection_preview_result import ProjectionPreviewResult


T = TypeVar("T", bound="ProjectionPreview")


@_attrs_define
class ProjectionPreview:
    """
    Attributes:
        bytes_after (int):
        bytes_before (int):
        changed (bool): Whether the projection changed anything.
        result (ProjectionPreviewResult): The result as the agent would be given it.
        truncated (bool): Whether a limit cut something.
    """

    bytes_after: int
    bytes_before: int
    changed: bool
    result: ProjectionPreviewResult
    truncated: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        bytes_after = self.bytes_after

        bytes_before = self.bytes_before

        changed = self.changed

        result = self.result.to_dict()

        truncated = self.truncated

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "bytes_after": bytes_after,
                "bytes_before": bytes_before,
                "changed": changed,
                "result": result,
                "truncated": truncated,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.projection_preview_result import (
            ProjectionPreviewResult,
        )

        d = dict(src_dict)
        bytes_after = d.pop("bytes_after")

        bytes_before = d.pop("bytes_before")

        changed = d.pop("changed")

        result = ProjectionPreviewResult.from_dict(d.pop("result"))

        truncated = d.pop("truncated")

        projection_preview = cls(
            bytes_after=bytes_after,
            bytes_before=bytes_before,
            changed=changed,
            result=result,
            truncated=truncated,
        )

        projection_preview.additional_properties = d
        return projection_preview

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
