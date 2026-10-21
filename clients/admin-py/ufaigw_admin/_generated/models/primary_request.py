from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from typing_extensions import Self

T = TypeVar("T", bound="PrimaryRequest")


@_attrs_define
class PrimaryRequest:
    """
    Attributes:
        model_id (int):
        weight (int): Share of the traffic among the primaries, 1 to 1000.
    """

    model_id: int
    weight: int

    def to_dict(self) -> dict[str, Any]:
        model_id = self.model_id

        weight = self.weight

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "model_id": model_id,
                "weight": weight,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        model_id = d.pop("model_id")

        weight = d.pop("weight")

        primary_request = cls(
            model_id=model_id,
            weight=weight,
        )

        return primary_request
