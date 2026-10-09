from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.channel_rule import ChannelRule


T = TypeVar("T", bound="ChannelView")


@_attrs_define
class ChannelView:
    """A channel as `/api` shows it: the host of its URL, never the URL or the
    secret.

        Attributes:
            created_at (str):
            enabled (bool):
            id (int):
            kind (str):
            name (str):
            rules (list[ChannelRule]): The rules that send to this channel.
            url_host (str): Scheme, host and port of the URL, like `https://hooks.slack.com`.
    """

    created_at: str
    enabled: bool
    id: int
    kind: str
    name: str
    rules: list[ChannelRule]
    url_host: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        created_at = self.created_at

        enabled = self.enabled

        id = self.id

        kind = self.kind

        name = self.name

        rules = []
        for rules_item_data in self.rules:
            rules_item = rules_item_data.to_dict()
            rules.append(rules_item)

        url_host = self.url_host

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "created_at": created_at,
                "enabled": enabled,
                "id": id,
                "kind": kind,
                "name": name,
                "rules": rules,
                "url_host": url_host,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.channel_rule import ChannelRule

        d = dict(src_dict)
        created_at = d.pop("created_at")

        enabled = d.pop("enabled")

        id = d.pop("id")

        kind = d.pop("kind")

        name = d.pop("name")

        rules = []
        _rules = d.pop("rules")
        for rules_item_data in _rules:
            rules_item = ChannelRule.from_dict(rules_item_data)

            rules.append(rules_item)

        url_host = d.pop("url_host")

        channel_view = cls(
            created_at=created_at,
            enabled=enabled,
            id=id,
            kind=kind,
            name=name,
            rules=rules,
            url_host=url_host,
        )

        channel_view.additional_properties = d
        return channel_view

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
