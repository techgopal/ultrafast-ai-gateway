from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="ProjectionEntry")


@_attrs_define
class ProjectionEntry:
    """A projection of a tool's result, as the file holds it.

    Attributes:
        max_bytes (int | None):
        max_items (int | None):
        drop (list[str] | Unset): JSONPath queries of what to remove, after `keep`.
        keep (list[str] | Unset): JSONPath queries of what to keep.
    """

    max_bytes: int | None
    max_items: int | None
    drop: list[str] | Unset = UNSET
    keep: list[str] | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        max_bytes: int | None
        max_bytes = self.max_bytes

        max_items: int | None
        max_items = self.max_items

        drop: list[str] | Unset = UNSET
        if not isinstance(self.drop, Unset):
            drop = self.drop

        keep: list[str] | Unset = UNSET
        if not isinstance(self.keep, Unset):
            keep = self.keep

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "max_bytes": max_bytes,
                "max_items": max_items,
            }
        )
        if drop is not UNSET:
            field_dict["drop"] = drop
        if keep is not UNSET:
            field_dict["keep"] = keep

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_max_bytes(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        max_bytes = _parse_max_bytes(d.pop("max_bytes"))

        def _parse_max_items(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        max_items = _parse_max_items(d.pop("max_items"))

        drop = cast(list[str], d.pop("drop", UNSET))

        keep = cast(list[str], d.pop("keep", UNSET))

        projection_entry = cls(
            max_bytes=max_bytes,
            max_items=max_items,
            drop=drop,
            keep=keep,
        )

        return projection_entry
