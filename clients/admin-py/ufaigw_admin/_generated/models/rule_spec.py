from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from typing_extensions import Self

from ..models.action import Action
from ..models.directions import Directions

if TYPE_CHECKING:
    from ..models.matcher_type_0 import MatcherType0
    from ..models.matcher_type_1 import MatcherType1
    from ..models.matcher_type_2 import MatcherType2


T = TypeVar("T", bound="RuleSpec")


@_attrs_define
class RuleSpec:
    """One rule of a guardrail.

    Attributes:
        action (Action):
        directions (Directions): The directions a rule applies to.
        id (str): Stable within a guardrail; the label of keyword/regex redaction counts.
        matcher (MatcherType0 | MatcherType1 | MatcherType2): What a rule looks for. Written as `{"keywords": {"words":
            [...],
            "whole_word": true}}`, `{"regex": "..."}` or `{"pii": ["EMAIL", ...]}`.
    """

    action: Action
    directions: Directions
    id: str
    matcher: MatcherType0 | MatcherType1 | MatcherType2

    def to_dict(self) -> dict[str, Any]:
        from ..models.matcher_type_0 import MatcherType0
        from ..models.matcher_type_1 import MatcherType1

        action = self.action.value

        directions = self.directions.value

        id = self.id

        matcher: dict[str, Any]
        if isinstance(self.matcher, MatcherType0) or isinstance(
            self.matcher, MatcherType1
        ):
            matcher = self.matcher.to_dict()
        else:
            matcher = self.matcher.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "action": action,
                "directions": directions,
                "id": id,
                "matcher": matcher,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.matcher_type_0 import MatcherType0
        from ..models.matcher_type_1 import MatcherType1
        from ..models.matcher_type_2 import MatcherType2

        d = dict(src_dict)
        action = Action(d.pop("action"))

        directions = Directions(d.pop("directions"))

        id = d.pop("id")

        def _parse_matcher(data: object) -> MatcherType0 | MatcherType1 | MatcherType2:
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_matcher_type_0 = MatcherType0.from_dict(data)

                return componentsschemas_matcher_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_matcher_type_1 = MatcherType1.from_dict(data)

                return componentsschemas_matcher_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            componentsschemas_matcher_type_2 = MatcherType2.from_dict(data)

            return componentsschemas_matcher_type_2

        matcher = _parse_matcher(d.pop("matcher"))

        rule_spec = cls(
            action=action,
            directions=directions,
            id=id,
            matcher=matcher,
        )

        return rule_spec
