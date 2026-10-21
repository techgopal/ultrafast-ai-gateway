from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="CreateChannelRequest")


@_attrs_define
class CreateChannelRequest:
    """
    Attributes:
        kind (str): `webhook` (the gateway's own JSON) or `slack` (`{"text": ...}`, which
            Slack and compatible incoming webhooks read).
        name (str):
        url (str): Where to post. A query string is allowed. Kept encrypted and never
            shown again; only its scheme, host and port are.
        enabled (bool | None | Unset): On when left out.
    """

    kind: str
    name: str
    url: str
    enabled: bool | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind

        name = self.name

        url = self.url

        enabled: bool | None | Unset
        if isinstance(self.enabled, Unset):
            enabled = UNSET
        else:
            enabled = self.enabled

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "kind": kind,
                "name": name,
                "url": url,
            }
        )
        if enabled is not UNSET:
            field_dict["enabled"] = enabled

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        kind = d.pop("kind")

        name = d.pop("name")

        url = d.pop("url")

        def _parse_enabled(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        enabled = _parse_enabled(d.pop("enabled", UNSET))

        create_channel_request = cls(
            kind=kind,
            name=name,
            url=url,
            enabled=enabled,
        )

        return create_channel_request
