from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.external_entry import ExternalEntry
    from ..models.rule_spec import RuleSpec


T = TypeVar("T", bound="GuardrailEntry")


@_attrs_define
class GuardrailEntry:
    """A guardrail. The URL and the signing secret of an external one are never
    in a file; one an import creates is off until its URL is set.

        Attributes:
            kind (str): `rules` or `external`.
            name (str):
            description (str | Unset):
            enabled (bool | Unset):
            external (ExternalEntry | None | Unset):
            is_default (bool | Unset): Applies to every call of the gateway.
            rules (list[RuleSpec] | Unset): The rules of a `rules` guardrail; empty for an external one.
    """

    kind: str
    name: str
    description: str | Unset = UNSET
    enabled: bool | Unset = UNSET
    external: ExternalEntry | None | Unset = UNSET
    is_default: bool | Unset = UNSET
    rules: list[RuleSpec] | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        from ..models.external_entry import ExternalEntry

        kind = self.kind

        name = self.name

        description = self.description

        enabled = self.enabled

        external: dict[str, Any] | None | Unset
        if isinstance(self.external, Unset):
            external = UNSET
        elif isinstance(self.external, ExternalEntry):
            external = self.external.to_dict()
        else:
            external = self.external

        is_default = self.is_default

        rules: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.rules, Unset):
            rules = []
            for rules_item_data in self.rules:
                rules_item = rules_item_data.to_dict()
                rules.append(rules_item)

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "kind": kind,
                "name": name,
            }
        )
        if description is not UNSET:
            field_dict["description"] = description
        if enabled is not UNSET:
            field_dict["enabled"] = enabled
        if external is not UNSET:
            field_dict["external"] = external
        if is_default is not UNSET:
            field_dict["is_default"] = is_default
        if rules is not UNSET:
            field_dict["rules"] = rules

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.external_entry import ExternalEntry
        from ..models.rule_spec import RuleSpec

        d = dict(src_dict)
        kind = d.pop("kind")

        name = d.pop("name")

        description = d.pop("description", UNSET)

        enabled = d.pop("enabled", UNSET)

        def _parse_external(data: object) -> ExternalEntry | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                external_type_0 = ExternalEntry.from_dict(data)

                return external_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(ExternalEntry | None | Unset, data)

        external = _parse_external(d.pop("external", UNSET))

        is_default = d.pop("is_default", UNSET)

        _rules = d.pop("rules", UNSET)
        rules: list[RuleSpec] | Unset = UNSET
        if _rules is not UNSET:
            rules = []
            for rules_item_data in _rules:
                rules_item = RuleSpec.from_dict(rules_item_data)

                rules.append(rules_item)

        guardrail_entry = cls(
            kind=kind,
            name=name,
            description=description,
            enabled=enabled,
            external=external,
            is_default=is_default,
            rules=rules,
        )

        return guardrail_entry
