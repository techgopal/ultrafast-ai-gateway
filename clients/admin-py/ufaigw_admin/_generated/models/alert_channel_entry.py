from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from typing_extensions import Self

T = TypeVar("T", bound="AlertChannelEntry")


@_attrs_define
class AlertChannelEntry:
    """An alert channel: its name and kind only. Its URL and secret are never in
    a file; a channel an import creates is off until its URL is set.

        Attributes:
            kind (str): `webhook` or `slack`.
            name (str):
    """

    kind: str
    name: str

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind

        name = self.name

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "kind": kind,
                "name": name,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        kind = d.pop("kind")

        name = d.pop("name")

        alert_channel_entry = cls(
            kind=kind,
            name=name,
        )

        return alert_channel_entry
