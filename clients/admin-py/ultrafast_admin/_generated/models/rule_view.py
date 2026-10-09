from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.channel_rule import ChannelRule
    from ..models.firing import Firing
    from ..models.rule_view_params import RuleViewParams


T = TypeVar("T", bound="RuleView")


@_attrs_define
class RuleView:
    """A rule as `/api` shows it.

    Attributes:
        channels (list[ChannelRule]): The channels it sends to.
        created_at (str):
        enabled (bool):
        firing (list[Firing]): What it is firing for now.
        id (int):
        kind (str):
        name (str):
        params (RuleViewParams): The parameters with every default written out.
    """

    channels: list[ChannelRule]
    created_at: str
    enabled: bool
    firing: list[Firing]
    id: int
    kind: str
    name: str
    params: RuleViewParams
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        channels = []
        for channels_item_data in self.channels:
            channels_item = channels_item_data.to_dict()
            channels.append(channels_item)

        created_at = self.created_at

        enabled = self.enabled

        firing = []
        for firing_item_data in self.firing:
            firing_item = firing_item_data.to_dict()
            firing.append(firing_item)

        id = self.id

        kind = self.kind

        name = self.name

        params = self.params.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "channels": channels,
                "created_at": created_at,
                "enabled": enabled,
                "firing": firing,
                "id": id,
                "kind": kind,
                "name": name,
                "params": params,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.channel_rule import ChannelRule
        from ..models.firing import Firing
        from ..models.rule_view_params import RuleViewParams

        d = dict(src_dict)
        channels = []
        _channels = d.pop("channels")
        for channels_item_data in _channels:
            channels_item = ChannelRule.from_dict(channels_item_data)

            channels.append(channels_item)

        created_at = d.pop("created_at")

        enabled = d.pop("enabled")

        firing = []
        _firing = d.pop("firing")
        for firing_item_data in _firing:
            firing_item = Firing.from_dict(firing_item_data)

            firing.append(firing_item)

        id = d.pop("id")

        kind = d.pop("kind")

        name = d.pop("name")

        params = RuleViewParams.from_dict(d.pop("params"))

        rule_view = cls(
            channels=channels,
            created_at=created_at,
            enabled=enabled,
            firing=firing,
            id=id,
            kind=kind,
            name=name,
            params=params,
        )

        rule_view.additional_properties = d
        return rule_view

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
