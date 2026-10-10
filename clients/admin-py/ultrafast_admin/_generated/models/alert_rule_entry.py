from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.alert_rule_entry_params import AlertRuleEntryParams


T = TypeVar("T", bound="AlertRuleEntry")


@_attrs_define
class AlertRuleEntry:
    """An alert rule. Its channels are named; a `budget` rule names its budget by
    `{scope, name, period}` (or `null`: every budget) instead of an id.

        Attributes:
            kind (str): `budget`, `error_rate` or `circuit_open`.
            name (str):
            params (AlertRuleEntryParams): As the API takes them, except that a `budget` rule has
                `{"budget": {"scope", "name", "period"} or null, "percent"}`.
            channels (list[str] | Unset): Names of channels.
            enabled (bool | Unset):
    """

    kind: str
    name: str
    params: AlertRuleEntryParams
    channels: list[str] | Unset = UNSET
    enabled: bool | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind

        name = self.name

        params = self.params.to_dict()

        channels: list[str] | Unset = UNSET
        if not isinstance(self.channels, Unset):
            channels = self.channels

        enabled = self.enabled

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "kind": kind,
                "name": name,
                "params": params,
            }
        )
        if channels is not UNSET:
            field_dict["channels"] = channels
        if enabled is not UNSET:
            field_dict["enabled"] = enabled

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.alert_rule_entry_params import (
            AlertRuleEntryParams,
        )

        d = dict(src_dict)
        kind = d.pop("kind")

        name = d.pop("name")

        params = AlertRuleEntryParams.from_dict(d.pop("params"))

        channels = cast(list[str], d.pop("channels", UNSET))

        enabled = d.pop("enabled", UNSET)

        alert_rule_entry = cls(
            kind=kind,
            name=name,
            params=params,
            channels=channels,
            enabled=enabled,
        )

        return alert_rule_entry
