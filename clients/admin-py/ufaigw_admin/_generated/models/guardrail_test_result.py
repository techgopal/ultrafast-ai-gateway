from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.outcome_view import OutcomeView


T = TypeVar("T", bound="GuardrailTestResult")


@_attrs_define
class GuardrailTestResult:
    """
    Attributes:
        outcome (OutcomeView): What a check found. Holds counts and ids only, never matched text.
        redacted_text (str): The text as the guardrail leaves it: redacted, or unchanged when it
            is blocked or only flagged.
    """

    outcome: OutcomeView
    redacted_text: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        outcome = self.outcome.to_dict()

        redacted_text = self.redacted_text

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "outcome": outcome,
                "redacted_text": redacted_text,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.outcome_view import OutcomeView

        d = dict(src_dict)
        outcome = OutcomeView.from_dict(d.pop("outcome"))

        redacted_text = d.pop("redacted_text")

        guardrail_test_result = cls(
            outcome=outcome,
            redacted_text=redacted_text,
        )

        guardrail_test_result.additional_properties = d
        return guardrail_test_result

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
