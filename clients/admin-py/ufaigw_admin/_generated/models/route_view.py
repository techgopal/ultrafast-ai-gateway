from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.fallback_view import FallbackView
    from ..models.guardrail_ref import GuardrailRef
    from ..models.primary_view import PrimaryView


T = TypeVar("T", bound="RouteView")


@_attrs_define
class RouteView:
    """A route. For a caller who is not an admin, `model_id`, `weight`, every
    setting, `everyone` and `team_ids` are hidden: they read as 0, false or
    empty whatever they are. Only the names and flags of the targets are real.

        Attributes:
            breaker_failures (int):
            breaker_open_s (int):
            breaker_window_s (int):
            broken (bool): No target of the route is enabled, so it cannot serve a request.
            cache_enabled (bool): The route keeps answers. Hidden (false) for a non-admin.
            cache_scope (str): `team`, `key` or `user`. Hidden (`team`) for a non-admin.
            cache_ttl_s (int): Seconds an answer is kept. Hidden (0) for a non-admin.
            created_at (str):
            everyone (bool): Every user may use the route. Hidden (false) for a non-admin.
            fallbacks (list[FallbackView]):
            first_token_timeout_ms (int):
            guardrails (list[GuardrailRef]): The guardrails applied to calls of this route, in order. Hidden
                (empty) for a non-admin.
            id (int):
            name (str):
            primaries (list[PrimaryView]):
            retries (int):
            team_ids (list[int]): Hidden (empty) for a non-admin.
            total_timeout_ms (int):
    """

    breaker_failures: int
    breaker_open_s: int
    breaker_window_s: int
    broken: bool
    cache_enabled: bool
    cache_scope: str
    cache_ttl_s: int
    created_at: str
    everyone: bool
    fallbacks: list[FallbackView]
    first_token_timeout_ms: int
    guardrails: list[GuardrailRef]
    id: int
    name: str
    primaries: list[PrimaryView]
    retries: int
    team_ids: list[int]
    total_timeout_ms: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        breaker_failures = self.breaker_failures

        breaker_open_s = self.breaker_open_s

        breaker_window_s = self.breaker_window_s

        broken = self.broken

        cache_enabled = self.cache_enabled

        cache_scope = self.cache_scope

        cache_ttl_s = self.cache_ttl_s

        created_at = self.created_at

        everyone = self.everyone

        fallbacks = []
        for fallbacks_item_data in self.fallbacks:
            fallbacks_item = fallbacks_item_data.to_dict()
            fallbacks.append(fallbacks_item)

        first_token_timeout_ms = self.first_token_timeout_ms

        guardrails = []
        for guardrails_item_data in self.guardrails:
            guardrails_item = guardrails_item_data.to_dict()
            guardrails.append(guardrails_item)

        id = self.id

        name = self.name

        primaries = []
        for primaries_item_data in self.primaries:
            primaries_item = primaries_item_data.to_dict()
            primaries.append(primaries_item)

        retries = self.retries

        team_ids = self.team_ids

        total_timeout_ms = self.total_timeout_ms

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "breaker_failures": breaker_failures,
                "breaker_open_s": breaker_open_s,
                "breaker_window_s": breaker_window_s,
                "broken": broken,
                "cache_enabled": cache_enabled,
                "cache_scope": cache_scope,
                "cache_ttl_s": cache_ttl_s,
                "created_at": created_at,
                "everyone": everyone,
                "fallbacks": fallbacks,
                "first_token_timeout_ms": first_token_timeout_ms,
                "guardrails": guardrails,
                "id": id,
                "name": name,
                "primaries": primaries,
                "retries": retries,
                "team_ids": team_ids,
                "total_timeout_ms": total_timeout_ms,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.fallback_view import FallbackView
        from ..models.guardrail_ref import GuardrailRef
        from ..models.primary_view import PrimaryView

        d = dict(src_dict)
        breaker_failures = d.pop("breaker_failures")

        breaker_open_s = d.pop("breaker_open_s")

        breaker_window_s = d.pop("breaker_window_s")

        broken = d.pop("broken")

        cache_enabled = d.pop("cache_enabled")

        cache_scope = d.pop("cache_scope")

        cache_ttl_s = d.pop("cache_ttl_s")

        created_at = d.pop("created_at")

        everyone = d.pop("everyone")

        fallbacks = []
        _fallbacks = d.pop("fallbacks")
        for fallbacks_item_data in _fallbacks:
            fallbacks_item = FallbackView.from_dict(fallbacks_item_data)

            fallbacks.append(fallbacks_item)

        first_token_timeout_ms = d.pop("first_token_timeout_ms")

        guardrails = []
        _guardrails = d.pop("guardrails")
        for guardrails_item_data in _guardrails:
            guardrails_item = GuardrailRef.from_dict(guardrails_item_data)

            guardrails.append(guardrails_item)

        id = d.pop("id")

        name = d.pop("name")

        primaries = []
        _primaries = d.pop("primaries")
        for primaries_item_data in _primaries:
            primaries_item = PrimaryView.from_dict(primaries_item_data)

            primaries.append(primaries_item)

        retries = d.pop("retries")

        team_ids = cast(list[int], d.pop("team_ids"))

        total_timeout_ms = d.pop("total_timeout_ms")

        route_view = cls(
            breaker_failures=breaker_failures,
            breaker_open_s=breaker_open_s,
            breaker_window_s=breaker_window_s,
            broken=broken,
            cache_enabled=cache_enabled,
            cache_scope=cache_scope,
            cache_ttl_s=cache_ttl_s,
            created_at=created_at,
            everyone=everyone,
            fallbacks=fallbacks,
            first_token_timeout_ms=first_token_timeout_ms,
            guardrails=guardrails,
            id=id,
            name=name,
            primaries=primaries,
            retries=retries,
            team_ids=team_ids,
            total_timeout_ms=total_timeout_ms,
        )

        route_view.additional_properties = d
        return route_view

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
