from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="LimitView")


@_attrs_define
class LimitView:
    """The limits of one team, user, key or of the gateway. A limit that is
    `null` is not set.

        Attributes:
            concurrent (int | None):
            id (int):
            label (str): How a refusal names it: `gateway`, `team 'Platform'`,
                `user 'lena@example.com'` or `key 'ci'`.
            requests_per_minute (int | None):
            scope (str): `gateway`, `team`, `user` or `key`.
            scope_id (int | None): The id of the team, user or key; `null` for the gateway.
            tokens_per_minute (int | None):
    """

    concurrent: int | None
    id: int
    label: str
    requests_per_minute: int | None
    scope: str
    scope_id: int | None
    tokens_per_minute: int | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        concurrent: int | None
        concurrent = self.concurrent

        id = self.id

        label = self.label

        requests_per_minute: int | None
        requests_per_minute = self.requests_per_minute

        scope = self.scope

        scope_id: int | None
        scope_id = self.scope_id

        tokens_per_minute: int | None
        tokens_per_minute = self.tokens_per_minute

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "concurrent": concurrent,
                "id": id,
                "label": label,
                "requests_per_minute": requests_per_minute,
                "scope": scope,
                "scope_id": scope_id,
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

        id = d.pop("id")

        label = d.pop("label")

        def _parse_requests_per_minute(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        requests_per_minute = _parse_requests_per_minute(d.pop("requests_per_minute"))

        scope = d.pop("scope")

        def _parse_scope_id(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        scope_id = _parse_scope_id(d.pop("scope_id"))

        def _parse_tokens_per_minute(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        tokens_per_minute = _parse_tokens_per_minute(d.pop("tokens_per_minute"))

        limit_view = cls(
            concurrent=concurrent,
            id=id,
            label=label,
            requests_per_minute=requests_per_minute,
            scope=scope,
            scope_id=scope_id,
            tokens_per_minute=tokens_per_minute,
        )

        limit_view.additional_properties = d
        return limit_view

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
