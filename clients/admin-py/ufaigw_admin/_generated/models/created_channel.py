from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.channel_view import ChannelView


T = TypeVar("T", bound="CreatedChannel")


@_attrs_define
class CreatedChannel:
    """
    Attributes:
        channel (ChannelView): A channel as `/api` shows it: the host of its URL, never the URL or the
            secret.
        secret (str): The signing secret of the channel. It is shown once, in this answer,
            and cannot be read again.
    """

    channel: ChannelView
    secret: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        channel = self.channel.to_dict()

        secret = self.secret

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "channel": channel,
                "secret": secret,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.channel_view import ChannelView

        d = dict(src_dict)
        channel = ChannelView.from_dict(d.pop("channel"))

        secret = d.pop("secret")

        created_channel = cls(
            channel=channel,
            secret=secret,
        )

        created_channel.additional_properties = d
        return created_channel

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
