from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..models.directions import Directions
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.rule_spec import RuleSpec


T = TypeVar("T", bound="UpdateGuardrailRequest")


@_attrs_define
class UpdateGuardrailRequest:
    """
    Attributes:
        description (None | str | Unset):
        directions (Directions | None | Unset):
        enabled (bool | None | Unset):
        fail_mode (None | str | Unset):
        is_default (bool | None | Unset):
        name (None | str | Unset):
        rules (list[RuleSpec] | None | Unset): `rules` only: replaces all the rules.
        timeout_ms (int | None | Unset):
        url (None | str | Unset): `external` only: a new URL.
    """

    description: None | str | Unset = UNSET
    directions: Directions | None | Unset = UNSET
    enabled: bool | None | Unset = UNSET
    fail_mode: None | str | Unset = UNSET
    is_default: bool | None | Unset = UNSET
    name: None | str | Unset = UNSET
    rules: list[RuleSpec] | None | Unset = UNSET
    timeout_ms: int | None | Unset = UNSET
    url: None | str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        description: None | str | Unset
        if isinstance(self.description, Unset):
            description = UNSET
        else:
            description = self.description

        directions: None | str | Unset
        if isinstance(self.directions, Unset):
            directions = UNSET
        elif isinstance(self.directions, Directions):
            directions = self.directions.value
        else:
            directions = self.directions

        enabled: bool | None | Unset
        if isinstance(self.enabled, Unset):
            enabled = UNSET
        else:
            enabled = self.enabled

        fail_mode: None | str | Unset
        if isinstance(self.fail_mode, Unset):
            fail_mode = UNSET
        else:
            fail_mode = self.fail_mode

        is_default: bool | None | Unset
        if isinstance(self.is_default, Unset):
            is_default = UNSET
        else:
            is_default = self.is_default

        name: None | str | Unset
        if isinstance(self.name, Unset):
            name = UNSET
        else:
            name = self.name

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

        timeout_ms: int | None | Unset
        if isinstance(self.timeout_ms, Unset):
            timeout_ms = UNSET
        else:
            timeout_ms = self.timeout_ms

        url: None | str | Unset
        if isinstance(self.url, Unset):
            url = UNSET
        else:
            url = self.url

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if description is not UNSET:
            field_dict["description"] = description
        if directions is not UNSET:
            field_dict["directions"] = directions
        if enabled is not UNSET:
            field_dict["enabled"] = enabled
        if fail_mode is not UNSET:
            field_dict["fail_mode"] = fail_mode
        if is_default is not UNSET:
            field_dict["is_default"] = is_default
        if name is not UNSET:
            field_dict["name"] = name
        if rules is not UNSET:
            field_dict["rules"] = rules
        if timeout_ms is not UNSET:
            field_dict["timeout_ms"] = timeout_ms
        if url is not UNSET:
            field_dict["url"] = url

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.rule_spec import RuleSpec

        d = dict(src_dict)

        def _parse_description(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        description = _parse_description(d.pop("description", UNSET))

        def _parse_directions(data: object) -> Directions | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                directions_type_0 = Directions(data)

                return directions_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(Directions | None | Unset, data)

        directions = _parse_directions(d.pop("directions", UNSET))

        def _parse_enabled(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        enabled = _parse_enabled(d.pop("enabled", UNSET))

        def _parse_fail_mode(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        fail_mode = _parse_fail_mode(d.pop("fail_mode", UNSET))

        def _parse_is_default(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        is_default = _parse_is_default(d.pop("is_default", UNSET))

        def _parse_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        name = _parse_name(d.pop("name", UNSET))

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

        def _parse_timeout_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        timeout_ms = _parse_timeout_ms(d.pop("timeout_ms", UNSET))

        def _parse_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        url = _parse_url(d.pop("url", UNSET))

        update_guardrail_request = cls(
            description=description,
            directions=directions,
            enabled=enabled,
            fail_mode=fail_mode,
            is_default=is_default,
            name=name,
            rules=rules,
            timeout_ms=timeout_ms,
            url=url,
        )

        return update_guardrail_request
