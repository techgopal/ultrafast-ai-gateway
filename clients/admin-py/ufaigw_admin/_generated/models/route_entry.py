from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.primary_entry import PrimaryEntry


T = TypeVar("T", bound="RouteEntry")


@_attrs_define
class RouteEntry:
    """
    Attributes:
        breaker_failures (int):
        breaker_open_s (int):
        breaker_window_s (int):
        first_token_timeout_ms (int):
        name (str):
        primaries (list[PrimaryEntry]):
        retries (int):
        total_timeout_ms (int):
        cache_enabled (bool | Unset):
        cache_scope (str | Unset):
        cache_ttl_s (int | Unset):
        everyone (bool | Unset):
        fallbacks (list[str] | Unset): `provider/model`, in the order they are tried.
        guardrails (list[str] | None | Unset): Names of guardrails, in the order they apply. Left out of the file
            for a route that has none; a file that leaves it out does not change
            what is attached (`[]` takes them all off).
        teams (list[str] | Unset): Names of teams.
    """

    breaker_failures: int
    breaker_open_s: int
    breaker_window_s: int
    first_token_timeout_ms: int
    name: str
    primaries: list[PrimaryEntry]
    retries: int
    total_timeout_ms: int
    cache_enabled: bool | Unset = UNSET
    cache_scope: str | Unset = UNSET
    cache_ttl_s: int | Unset = UNSET
    everyone: bool | Unset = UNSET
    fallbacks: list[str] | Unset = UNSET
    guardrails: list[str] | None | Unset = UNSET
    teams: list[str] | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        breaker_failures = self.breaker_failures

        breaker_open_s = self.breaker_open_s

        breaker_window_s = self.breaker_window_s

        first_token_timeout_ms = self.first_token_timeout_ms

        name = self.name

        primaries = []
        for primaries_item_data in self.primaries:
            primaries_item = primaries_item_data.to_dict()
            primaries.append(primaries_item)

        retries = self.retries

        total_timeout_ms = self.total_timeout_ms

        cache_enabled = self.cache_enabled

        cache_scope = self.cache_scope

        cache_ttl_s = self.cache_ttl_s

        everyone = self.everyone

        fallbacks: list[str] | Unset = UNSET
        if not isinstance(self.fallbacks, Unset):
            fallbacks = self.fallbacks

        guardrails: list[str] | None | Unset
        if isinstance(self.guardrails, Unset):
            guardrails = UNSET
        elif isinstance(self.guardrails, list):
            guardrails = self.guardrails

        else:
            guardrails = self.guardrails

        teams: list[str] | Unset = UNSET
        if not isinstance(self.teams, Unset):
            teams = self.teams

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "breaker_failures": breaker_failures,
                "breaker_open_s": breaker_open_s,
                "breaker_window_s": breaker_window_s,
                "first_token_timeout_ms": first_token_timeout_ms,
                "name": name,
                "primaries": primaries,
                "retries": retries,
                "total_timeout_ms": total_timeout_ms,
            }
        )
        if cache_enabled is not UNSET:
            field_dict["cache_enabled"] = cache_enabled
        if cache_scope is not UNSET:
            field_dict["cache_scope"] = cache_scope
        if cache_ttl_s is not UNSET:
            field_dict["cache_ttl_s"] = cache_ttl_s
        if everyone is not UNSET:
            field_dict["everyone"] = everyone
        if fallbacks is not UNSET:
            field_dict["fallbacks"] = fallbacks
        if guardrails is not UNSET:
            field_dict["guardrails"] = guardrails
        if teams is not UNSET:
            field_dict["teams"] = teams

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.primary_entry import PrimaryEntry

        d = dict(src_dict)
        breaker_failures = d.pop("breaker_failures")

        breaker_open_s = d.pop("breaker_open_s")

        breaker_window_s = d.pop("breaker_window_s")

        first_token_timeout_ms = d.pop("first_token_timeout_ms")

        name = d.pop("name")

        primaries = []
        _primaries = d.pop("primaries")
        for primaries_item_data in _primaries:
            primaries_item = PrimaryEntry.from_dict(primaries_item_data)

            primaries.append(primaries_item)

        retries = d.pop("retries")

        total_timeout_ms = d.pop("total_timeout_ms")

        cache_enabled = d.pop("cache_enabled", UNSET)

        cache_scope = d.pop("cache_scope", UNSET)

        cache_ttl_s = d.pop("cache_ttl_s", UNSET)

        everyone = d.pop("everyone", UNSET)

        fallbacks = cast(list[str], d.pop("fallbacks", UNSET))

        def _parse_guardrails(data: object) -> list[str] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                guardrails_type_0 = cast(list[str], data)

                return guardrails_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None | Unset, data)

        guardrails = _parse_guardrails(d.pop("guardrails", UNSET))

        teams = cast(list[str], d.pop("teams", UNSET))

        route_entry = cls(
            breaker_failures=breaker_failures,
            breaker_open_s=breaker_open_s,
            breaker_window_s=breaker_window_s,
            first_token_timeout_ms=first_token_timeout_ms,
            name=name,
            primaries=primaries,
            retries=retries,
            total_timeout_ms=total_timeout_ms,
            cache_enabled=cache_enabled,
            cache_scope=cache_scope,
            cache_ttl_s=cache_ttl_s,
            everyone=everyone,
            fallbacks=fallbacks,
            guardrails=guardrails,
            teams=teams,
        )

        return route_entry
