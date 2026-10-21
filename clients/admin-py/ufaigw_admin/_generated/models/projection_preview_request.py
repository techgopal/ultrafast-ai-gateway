from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.projection_body import ProjectionBody
    from ..models.projection_preview_request_result import (
        ProjectionPreviewRequestResult,
    )


T = TypeVar("T", bound="ProjectionPreviewRequest")


@_attrs_define
class ProjectionPreviewRequest:
    """A result to project, and optionally a projection to try on it.

    Attributes:
        result (ProjectionPreviewRequestResult): A result of the tool as `tools/call` returns it (`content`,
            `structuredContent`), for instance the last one from the playground.
        projection (None | ProjectionBody | Unset):
    """

    result: ProjectionPreviewRequestResult
    projection: None | ProjectionBody | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        from ..models.projection_body import ProjectionBody

        result = self.result.to_dict()

        projection: dict[str, Any] | None | Unset
        if isinstance(self.projection, Unset):
            projection = UNSET
        elif isinstance(self.projection, ProjectionBody):
            projection = self.projection.to_dict()
        else:
            projection = self.projection

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "result": result,
            }
        )
        if projection is not UNSET:
            field_dict["projection"] = projection

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.projection_body import ProjectionBody
        from ..models.projection_preview_request_result import (
            ProjectionPreviewRequestResult,
        )

        d = dict(src_dict)
        result = ProjectionPreviewRequestResult.from_dict(d.pop("result"))

        def _parse_projection(data: object) -> None | ProjectionBody | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                projection_type_0 = ProjectionBody.from_dict(data)

                return projection_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | ProjectionBody | Unset, data)

        projection = _parse_projection(d.pop("projection", UNSET))

        projection_preview_request = cls(
            result=result,
            projection=projection,
        )

        return projection_preview_request
