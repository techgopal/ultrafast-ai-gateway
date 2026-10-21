from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..models.directions import Directions

if TYPE_CHECKING:
    from ..models.guardrail_ref import GuardrailRef
    from ..models.rule_spec import RuleSpec


T = TypeVar("T", bound="GuardrailView")


@_attrs_define
class GuardrailView:
    """A guardrail as `/api` shows it: the host of an external one's URL, never
    the URL or the secret.

        Attributes:
            created_at (str):
            description (str):
            directions (Directions | None):
            enabled (bool):
            fail_mode (None | str): `open` or `closed`; `external` only.
            id (int):
            is_default (bool): Applies to every call of the gateway.
            key_count (int): How many keys it is attached to.
            kind (str): `rules` or `external`.
            name (str):
            routes (list[GuardrailRef]): The routes it is attached to.
            rules (list[RuleSpec]): The rules of a `rules` guardrail; empty for an `external` one.
            timeout_ms (int | None): `external` only; `null` for `rules`.
            url_host (None | str): `external`: scheme, host and port of the URL, like
                `https://guard.example.com`; empty until a URL is set (a guardrail an
                import made). `null` for `rules`.
            usable (bool): Whether the gateway can use it as stored. `false` for an external
                guardrail whose URL or secret cannot be read (it fails by its mode on
                every check), and for an enabled rules guardrail whose rules the
                gateway is not running (they do not compile). A disabled one is `true`.
    """

    created_at: str
    description: str
    directions: Directions | None
    enabled: bool
    fail_mode: None | str
    id: int
    is_default: bool
    key_count: int
    kind: str
    name: str
    routes: list[GuardrailRef]
    rules: list[RuleSpec]
    timeout_ms: int | None
    url_host: None | str
    usable: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        created_at = self.created_at

        description = self.description

        directions: None | str
        if isinstance(self.directions, Directions):
            directions = self.directions.value
        else:
            directions = self.directions

        enabled = self.enabled

        fail_mode: None | str
        fail_mode = self.fail_mode

        id = self.id

        is_default = self.is_default

        key_count = self.key_count

        kind = self.kind

        name = self.name

        routes = []
        for routes_item_data in self.routes:
            routes_item = routes_item_data.to_dict()
            routes.append(routes_item)

        rules = []
        for rules_item_data in self.rules:
            rules_item = rules_item_data.to_dict()
            rules.append(rules_item)

        timeout_ms: int | None
        timeout_ms = self.timeout_ms

        url_host: None | str
        url_host = self.url_host

        usable = self.usable

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "created_at": created_at,
                "description": description,
                "directions": directions,
                "enabled": enabled,
                "fail_mode": fail_mode,
                "id": id,
                "is_default": is_default,
                "key_count": key_count,
                "kind": kind,
                "name": name,
                "routes": routes,
                "rules": rules,
                "timeout_ms": timeout_ms,
                "url_host": url_host,
                "usable": usable,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.guardrail_ref import GuardrailRef
        from ..models.rule_spec import RuleSpec

        d = dict(src_dict)
        created_at = d.pop("created_at")

        description = d.pop("description")

        def _parse_directions(data: object) -> Directions | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                directions_type_0 = Directions(data)

                return directions_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(Directions | None, data)

        directions = _parse_directions(d.pop("directions"))

        enabled = d.pop("enabled")

        def _parse_fail_mode(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        fail_mode = _parse_fail_mode(d.pop("fail_mode"))

        id = d.pop("id")

        is_default = d.pop("is_default")

        key_count = d.pop("key_count")

        kind = d.pop("kind")

        name = d.pop("name")

        routes = []
        _routes = d.pop("routes")
        for routes_item_data in _routes:
            routes_item = GuardrailRef.from_dict(routes_item_data)

            routes.append(routes_item)

        rules = []
        _rules = d.pop("rules")
        for rules_item_data in _rules:
            rules_item = RuleSpec.from_dict(rules_item_data)

            rules.append(rules_item)

        def _parse_timeout_ms(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        timeout_ms = _parse_timeout_ms(d.pop("timeout_ms"))

        def _parse_url_host(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        url_host = _parse_url_host(d.pop("url_host"))

        usable = d.pop("usable")

        guardrail_view = cls(
            created_at=created_at,
            description=description,
            directions=directions,
            enabled=enabled,
            fail_mode=fail_mode,
            id=id,
            is_default=is_default,
            key_count=key_count,
            kind=kind,
            name=name,
            routes=routes,
            rules=rules,
            timeout_ms=timeout_ms,
            url_host=url_host,
            usable=usable,
        )

        guardrail_view.additional_properties = d
        return guardrail_view

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
