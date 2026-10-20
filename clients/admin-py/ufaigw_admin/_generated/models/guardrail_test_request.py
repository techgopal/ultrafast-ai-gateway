from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..models.direction import Direction
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.rule_spec import RuleSpec


T = TypeVar("T", bound="GuardrailTestRequest")


@_attrs_define
class GuardrailTestRequest:
    """What a test sends: rules, or the id of a stored guardrail.

    Attributes:
        direction (Direction):
        text (str): Up to 20 000 characters.
        call_external (bool | Unset): Call the external guardrail `guardrail_id` for real, as a call would
            (signed, with its timeout and fail mode): the text is sent to its
            URL, and the outcome says what it decided or, in `flags`, why it
            could not (`external_error:<reason>`). The hook is asked only when
            the guardrail covers `direction`. Without this, an external guardrail
            is never called by a test, and sending its id is refused (422).
            Not for `rules`.
        guardrail_id (int | None | Unset): A stored guardrail, enabled or not.
        rules (list[RuleSpec] | None | Unset): Rules to try, as for a new guardrail. Send these or `guardrail_id`.
    """

    direction: Direction
    text: str
    call_external: bool | Unset = UNSET
    guardrail_id: int | None | Unset = UNSET
    rules: list[RuleSpec] | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        direction = self.direction.value

        text = self.text

        call_external = self.call_external

        guardrail_id: int | None | Unset
        if isinstance(self.guardrail_id, Unset):
            guardrail_id = UNSET
        else:
            guardrail_id = self.guardrail_id

        rules: list[dict[str, Any]] | None | Unset
        if isinstance(self.rules, Unset):
            rules = UNSET
        elif isinstance(self.rules, list):
            rules = []
            for rules_type_0_item_data in self.rules:
                rules_type_0_item = rules_type_0_item_data.to_dict()
                rules.append(rules_type_0_item)

        else:
            rules = self.rules

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "direction": direction,
                "text": text,
            }
        )
        if call_external is not UNSET:
            field_dict["call_external"] = call_external
        if guardrail_id is not UNSET:
            field_dict["guardrail_id"] = guardrail_id
        if rules is not UNSET:
            field_dict["rules"] = rules

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.rule_spec import RuleSpec

        d = dict(src_dict)
        direction = Direction(d.pop("direction"))

        text = d.pop("text")

        call_external = d.pop("call_external", UNSET)

        def _parse_guardrail_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        guardrail_id = _parse_guardrail_id(d.pop("guardrail_id", UNSET))

        def _parse_rules(data: object) -> list[RuleSpec] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                rules_type_0 = []
                _rules_type_0 = data
                for rules_type_0_item_data in _rules_type_0:
                    rules_type_0_item = RuleSpec.from_dict(rules_type_0_item_data)

                    rules_type_0.append(rules_type_0_item)

                return rules_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[RuleSpec] | None | Unset, data)

        rules = _parse_rules(d.pop("rules", UNSET))

        guardrail_test_request = cls(
            direction=direction,
            text=text,
            call_external=call_external,
            guardrail_id=guardrail_id,
            rules=rules,
        )

        return guardrail_test_request
