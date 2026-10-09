from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="FlagLog")


@_attrs_define
class FlagLog:
    """A flag rule that matched.

    Attributes:
        guardrail_id (int):
        rule_id (str):
    """

    guardrail_id: int
    rule_id: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        guardrail_id = self.guardrail_id

        rule_id = self.rule_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "guardrail_id": guardrail_id,
                "rule_id": rule_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        guardrail_id = d.pop("guardrail_id")

        rule_id = d.pop("rule_id")

        flag_log = cls(
            guardrail_id=guardrail_id,
            rule_id=rule_id,
        )

        flag_log.additional_properties = d
        return flag_log

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
