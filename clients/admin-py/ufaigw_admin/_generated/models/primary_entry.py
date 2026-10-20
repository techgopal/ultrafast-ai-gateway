from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from typing_extensions import Self

T = TypeVar("T", bound="PrimaryEntry")


@_attrs_define
class PrimaryEntry:
    """
    Attributes:
        model (str): `provider/model`.
        weight (int):
    """

    model: str
    weight: int

    def to_dict(self) -> dict[str, Any]:
        model = self.model

        weight = self.weight

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "model": model,
                "weight": weight,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        model = d.pop("model")

        weight = d.pop("weight")

        primary_entry = cls(
            model=model,
            weight=weight,
        )

        return primary_entry
