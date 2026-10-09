from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="SetLimitRequest")


@_attrs_define
class SetLimitRequest:
    """
    Attributes:
        scope (str): `gateway`, `team`, `user` or `key`.
        concurrent (int | None | Unset): 1 to 1 000 000. Not sent: no limit.
        requests_per_minute (int | None | Unset): 1 to 1 000 000. Not sent: no limit.
        scope_id (int | None | Unset): The id of the team, user or key. Not sent for the gateway.
        tokens_per_minute (int | None | Unset): 1 to 1 000 000 000 000. Not sent: no limit.
    """

    scope: str
    concurrent: int | None | Unset = UNSET
    requests_per_minute: int | None | Unset = UNSET
    scope_id: int | None | Unset = UNSET
    tokens_per_minute: int | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        scope = self.scope

        concurrent: int | None | Unset
        if isinstance(self.concurrent, Unset):
            concurrent = UNSET
        else:
            concurrent = self.concurrent

        requests_per_minute: int | None | Unset
        if isinstance(self.requests_per_minute, Unset):
            requests_per_minute = UNSET
        else:
            requests_per_minute = self.requests_per_minute

        scope_id: int | None | Unset
        if isinstance(self.scope_id, Unset):
            scope_id = UNSET
        else:
            scope_id = self.scope_id

        tokens_per_minute: int | None | Unset
        if isinstance(self.tokens_per_minute, Unset):
            tokens_per_minute = UNSET
        else:
            tokens_per_minute = self.tokens_per_minute

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "scope": scope,
            }
        )
        if concurrent is not UNSET:
            field_dict["concurrent"] = concurrent
        if requests_per_minute is not UNSET:
            field_dict["requests_per_minute"] = requests_per_minute
        if scope_id is not UNSET:
            field_dict["scope_id"] = scope_id
        if tokens_per_minute is not UNSET:
            field_dict["tokens_per_minute"] = tokens_per_minute

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        scope = d.pop("scope")

        def _parse_concurrent(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        concurrent = _parse_concurrent(d.pop("concurrent", UNSET))

        def _parse_requests_per_minute(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        requests_per_minute = _parse_requests_per_minute(
            d.pop("requests_per_minute", UNSET)
        )

        def _parse_scope_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        scope_id = _parse_scope_id(d.pop("scope_id", UNSET))

        def _parse_tokens_per_minute(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        tokens_per_minute = _parse_tokens_per_minute(d.pop("tokens_per_minute", UNSET))

        set_limit_request = cls(
            scope=scope,
            concurrent=concurrent,
            requests_per_minute=requests_per_minute,
            scope_id=scope_id,
            tokens_per_minute=tokens_per_minute,
        )

        return set_limit_request
