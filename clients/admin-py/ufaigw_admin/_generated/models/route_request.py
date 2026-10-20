from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.primary_request import PrimaryRequest


T = TypeVar("T", bound="RouteRequest")


@_attrs_define
class RouteRequest:
    """
    Attributes:
        breaker_failures (int): 1 to 100.
        breaker_open_s (int): 5 to 3 600.
        breaker_window_s (int): 5 to 3 600.
        everyone (bool): Every user may use the route. It cannot be combined with teams.
            Without it and without teams only admins may use the route.
        fallbacks (list[int]): Model ids, tried in this order when the primaries fail.
        first_token_timeout_ms (int): 1 000 to 300 000.
        name (str): 1 to 64 characters of `a-z`, `0-9`, `.`, `_`, `-`, starting with a
            letter or digit. No `/`, so a route never reads as `provider/model`.
        primaries (list[PrimaryRequest]): At least one. A model appears at most once in the route.
        retries (int): 0 to 5.
        team_ids (list[int]): Teams that may use the route.
        total_timeout_ms (int): 1 000 to 3 600 000, not below the first token timeout.
        cache_enabled (bool | Unset): Keep the answers of calls to this route and give them again to the
            same call, without a provider. Streams and calls with a temperature
            above 0.5 are never kept. Not sent: off.
        cache_scope (str | Unset): Whom a kept answer is given to: `team` (the team of the key; a key
            with no team uses `user`, then `key`), `key` or `user`. Never across
            teams. Not sent: `team`.
        cache_ttl_s (int | Unset): How long an answer is kept, 1 to 86 400 seconds. Not sent: 300.
        guardrail_ids (list[int] | None | Unset): The guardrails applied to calls of this route, in this order, after
            the gateway-wide ones. At most 20. Left out, the route keeps the ones
            it has; `[]` takes them all off.
    """

    breaker_failures: int
    breaker_open_s: int
    breaker_window_s: int
    everyone: bool
    fallbacks: list[int]
    first_token_timeout_ms: int
    name: str
    primaries: list[PrimaryRequest]
    retries: int
    team_ids: list[int]
    total_timeout_ms: int
    cache_enabled: bool | Unset = UNSET
    cache_scope: str | Unset = UNSET
    cache_ttl_s: int | Unset = UNSET
    guardrail_ids: list[int] | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        breaker_failures = self.breaker_failures

        breaker_open_s = self.breaker_open_s

        breaker_window_s = self.breaker_window_s

        everyone = self.everyone

        fallbacks = self.fallbacks

        first_token_timeout_ms = self.first_token_timeout_ms

        name = self.name

        primaries = []
        for primaries_item_data in self.primaries:
            primaries_item = primaries_item_data.to_dict()
            primaries.append(primaries_item)

        retries = self.retries

        team_ids = self.team_ids

        total_timeout_ms = self.total_timeout_ms

        cache_enabled = self.cache_enabled

        cache_scope = self.cache_scope

        cache_ttl_s = self.cache_ttl_s

        guardrail_ids: list[int] | None | Unset
        if isinstance(self.guardrail_ids, Unset):
            guardrail_ids = UNSET
        elif isinstance(self.guardrail_ids, list):
            guardrail_ids = self.guardrail_ids

        else:
            guardrail_ids = self.guardrail_ids

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "breaker_failures": breaker_failures,
                "breaker_open_s": breaker_open_s,
                "breaker_window_s": breaker_window_s,
                "everyone": everyone,
                "fallbacks": fallbacks,
                "first_token_timeout_ms": first_token_timeout_ms,
                "name": name,
                "primaries": primaries,
                "retries": retries,
                "team_ids": team_ids,
                "total_timeout_ms": total_timeout_ms,
            }
        )
        if cache_enabled is not UNSET:
            field_dict["cache_enabled"] = cache_enabled
        if cache_scope is not UNSET:
            field_dict["cache_scope"] = cache_scope
        if cache_ttl_s is not UNSET:
            field_dict["cache_ttl_s"] = cache_ttl_s
        if guardrail_ids is not UNSET:
            field_dict["guardrail_ids"] = guardrail_ids

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.primary_request import PrimaryRequest

        d = dict(src_dict)
        breaker_failures = d.pop("breaker_failures")

        breaker_open_s = d.pop("breaker_open_s")

        breaker_window_s = d.pop("breaker_window_s")

        everyone = d.pop("everyone")

        fallbacks = cast(list[int], d.pop("fallbacks"))

        first_token_timeout_ms = d.pop("first_token_timeout_ms")

        name = d.pop("name")

        primaries = []
        _primaries = d.pop("primaries")
        for primaries_item_data in _primaries:
            primaries_item = PrimaryRequest.from_dict(primaries_item_data)

            primaries.append(primaries_item)

        retries = d.pop("retries")

        team_ids = cast(list[int], d.pop("team_ids"))

        total_timeout_ms = d.pop("total_timeout_ms")

        cache_enabled = d.pop("cache_enabled", UNSET)

        cache_scope = d.pop("cache_scope", UNSET)

        cache_ttl_s = d.pop("cache_ttl_s", UNSET)

        def _parse_guardrail_ids(data: object) -> list[int] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                guardrail_ids_type_0 = cast(list[int], data)

                return guardrail_ids_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[int] | None | Unset, data)

        guardrail_ids = _parse_guardrail_ids(d.pop("guardrail_ids", UNSET))

        route_request = cls(
            breaker_failures=breaker_failures,
            breaker_open_s=breaker_open_s,
            breaker_window_s=breaker_window_s,
            everyone=everyone,
            fallbacks=fallbacks,
            first_token_timeout_ms=first_token_timeout_ms,
            name=name,
            primaries=primaries,
            retries=retries,
            team_ids=team_ids,
            total_timeout_ms=total_timeout_ms,
            cache_enabled=cache_enabled,
            cache_scope=cache_scope,
            cache_ttl_s=cache_ttl_s,
            guardrail_ids=guardrail_ids,
        )

        return route_request
