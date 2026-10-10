from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.create_rule_request_params import CreateRuleRequestParams


T = TypeVar("T", bound="CreateRuleRequest")


@_attrs_define
class CreateRuleRequest:
    """
    Attributes:
        channel_ids (list[int]): The channels that are told, by id.
        kind (str): `budget`, `error_rate` or `circuit_open`.
        name (str):
        params (CreateRuleRequestParams): By kind. `budget`: `{"budget_id": id or null, "percent": 1-100}`.
            `error_rate`: `{"scope": "gateway"|"route"|"provider"|"key",
            "subject": name or null, "percent": 1-100, "window_minutes": 5-60
            (5), "min_requests": 1-100000 (20)}`. `circuit_open`:
            `{"provider": name or null, "model": name or null}`. Unknown fields
            are refused.
        enabled (bool | None | Unset): On when left out.
    """

    channel_ids: list[int]
    kind: str
    name: str
    params: CreateRuleRequestParams
    enabled: bool | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        channel_ids = self.channel_ids

        kind = self.kind

        name = self.name

        params = self.params.to_dict()

        enabled: bool | None | Unset
        if isinstance(self.enabled, Unset):
            enabled = UNSET
        else:
            enabled = self.enabled

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "channel_ids": channel_ids,
                "kind": kind,
                "name": name,
                "params": params,
            }
        )
        if enabled is not UNSET:
            field_dict["enabled"] = enabled

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.create_rule_request_params import (
            CreateRuleRequestParams,
        )

        d = dict(src_dict)
        channel_ids = cast(list[int], d.pop("channel_ids"))

        kind = d.pop("kind")

        name = d.pop("name")

        params = CreateRuleRequestParams.from_dict(d.pop("params"))

        def _parse_enabled(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        enabled = _parse_enabled(d.pop("enabled", UNSET))

        create_rule_request = cls(
            channel_ids=channel_ids,
            kind=kind,
            name=name,
            params=params,
            enabled=enabled,
        )

        return create_rule_request
