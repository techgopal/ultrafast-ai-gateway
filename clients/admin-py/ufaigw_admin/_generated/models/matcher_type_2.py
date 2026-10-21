from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..models.pii_type import PiiType

T = TypeVar("T", bound="MatcherType2")


@_attrs_define
class MatcherType2:
    """
    Attributes:
        pii (list[PiiType]):
    """

    pii: list[PiiType]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        pii = []
        for pii_item_data in self.pii:
            pii_item = pii_item_data.value
            pii.append(pii_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "pii": pii,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        pii = []
        _pii = d.pop("pii")
        for pii_item_data in _pii:
            pii_item = PiiType(pii_item_data)

            pii.append(pii_item)

        matcher_type_2 = cls(
            pii=pii,
        )

        matcher_type_2.additional_properties = d
        return matcher_type_2

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
