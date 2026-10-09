from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.guardrail_view import GuardrailView


T = TypeVar("T", bound="GuardrailList")


@_attrs_define
class GuardrailList:
    """
    Attributes:
        guardrails (list[GuardrailView]):
    """

    guardrails: list[GuardrailView]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        guardrails = []
        for guardrails_item_data in self.guardrails:
            guardrails_item = guardrails_item_data.to_dict()
            guardrails.append(guardrails_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "guardrails": guardrails,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.guardrail_view import GuardrailView

        d = dict(src_dict)
        guardrails = []
        _guardrails = d.pop("guardrails")
        for guardrails_item_data in _guardrails:
            guardrails_item = GuardrailView.from_dict(guardrails_item_data)

            guardrails.append(guardrails_item)

        guardrail_list = cls(
            guardrails=guardrails,
        )

        guardrail_list.additional_properties = d
        return guardrail_list

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
