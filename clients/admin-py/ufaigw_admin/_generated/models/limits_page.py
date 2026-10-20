from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.limit_view import LimitView


T = TypeVar("T", bound="LimitsPage")


@_attrs_define
class LimitsPage:
    """
    Attributes:
        limits (list[LimitView]):
    """

    limits: list[LimitView]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        limits = []
        for limits_item_data in self.limits:
            limits_item = limits_item_data.to_dict()
            limits.append(limits_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "limits": limits,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.limit_view import LimitView

        d = dict(src_dict)
        limits = []
        _limits = d.pop("limits")
        for limits_item_data in _limits:
            limits_item = LimitView.from_dict(limits_item_data)

            limits.append(limits_item)

        limits_page = cls(
            limits=limits,
        )

        limits_page.additional_properties = d
        return limits_page

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
