from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.flag_view import FlagView
    from ..models.guardrail_ref import GuardrailRef
    from ..models.outcome_view_redactions import OutcomeViewRedactions


T = TypeVar("T", bound="OutcomeView")


@_attrs_define
class OutcomeView:
    """What a check found. Holds counts and ids only, never matched text.

    Attributes:
        blocked_by (GuardrailRef | None):
        flags (list[FlagView]):
        redactions (OutcomeViewRedactions): Replacements made, by PII type (`EMAIL`) or by rule id.
    """

    blocked_by: GuardrailRef | None
    flags: list[FlagView]
    redactions: OutcomeViewRedactions
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.guardrail_ref import GuardrailRef

        blocked_by: dict[str, Any] | None
        if isinstance(self.blocked_by, GuardrailRef):
            blocked_by = self.blocked_by.to_dict()
        else:
            blocked_by = self.blocked_by

        flags = []
        for flags_item_data in self.flags:
            flags_item = flags_item_data.to_dict()
            flags.append(flags_item)

        redactions = self.redactions.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "blocked_by": blocked_by,
                "flags": flags,
                "redactions": redactions,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.flag_view import FlagView
        from ..models.guardrail_ref import GuardrailRef
        from ..models.outcome_view_redactions import (
            OutcomeViewRedactions,
        )

        d = dict(src_dict)

        def _parse_blocked_by(data: object) -> GuardrailRef | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                blocked_by_type_0 = GuardrailRef.from_dict(data)

                return blocked_by_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(GuardrailRef | None, data)

        blocked_by = _parse_blocked_by(d.pop("blocked_by"))

        flags = []
        _flags = d.pop("flags")
        for flags_item_data in _flags:
            flags_item = FlagView.from_dict(flags_item_data)

            flags.append(flags_item)

        redactions = OutcomeViewRedactions.from_dict(d.pop("redactions"))

        outcome_view = cls(
            blocked_by=blocked_by,
            flags=flags,
            redactions=redactions,
        )

        outcome_view.additional_properties = d
        return outcome_view

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
