from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

T = TypeVar("T", bound="LimitEntry")


@_attrs_define
class LimitEntry:
    """
    Attributes:
        concurrent (int | None):
        name (None | str): The team's name or the user's email; `null` for the gateway.
        requests_per_minute (int | None):
        scope (str): `gateway`, `team` or `user`.
        tokens_per_minute (int | None):
    """

    concurrent: int | None
    name: None | str
    requests_per_minute: int | None
    scope: str
    tokens_per_minute: int | None

    def to_dict(self) -> dict[str, Any]:
        concurrent: int | None
        concurrent = self.concurrent

        name: None | str
        name = self.name

        requests_per_minute: int | None
        requests_per_minute = self.requests_per_minute

        scope = self.scope

        tokens_per_minute: int | None
        tokens_per_minute = self.tokens_per_minute

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "concurrent": concurrent,
                "name": name,
                "requests_per_minute": requests_per_minute,
                "scope": scope,
                "tokens_per_minute": tokens_per_minute,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_concurrent(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        concurrent = _parse_concurrent(d.pop("concurrent"))

        def _parse_name(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        name = _parse_name(d.pop("name"))

        def _parse_requests_per_minute(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        requests_per_minute = _parse_requests_per_minute(d.pop("requests_per_minute"))

        scope = d.pop("scope")

        def _parse_tokens_per_minute(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        tokens_per_minute = _parse_tokens_per_minute(d.pop("tokens_per_minute"))

        limit_entry = cls(
            concurrent=concurrent,
            name=name,
            requests_per_minute=requests_per_minute,
            scope=scope,
            tokens_per_minute=tokens_per_minute,
        )

        return limit_entry
