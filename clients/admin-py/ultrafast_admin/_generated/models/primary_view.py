from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="PrimaryView")


@_attrs_define
class PrimaryView:
    """
    Attributes:
        enabled (bool): Whether the model is enabled.
        model (str): `provider_name/model_name`.
        model_id (int): Zero for a caller who is not an admin.
        weight (int): Zero for a caller who is not an admin.
    """

    enabled: bool
    model: str
    model_id: int
    weight: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        enabled = self.enabled

        model = self.model

        model_id = self.model_id

        weight = self.weight

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "enabled": enabled,
                "model": model,
                "model_id": model_id,
                "weight": weight,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        enabled = d.pop("enabled")

        model = d.pop("model")

        model_id = d.pop("model_id")

        weight = d.pop("weight")

        primary_view = cls(
            enabled=enabled,
            model=model,
            model_id=model_id,
            weight=weight,
        )

        primary_view.additional_properties = d
        return primary_view

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
