from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="PlaygroundConfig")


@_attrs_define
class PlaygroundConfig:
    """What the console's playground needs to know before it sends a call.

    Attributes:
        max_audio_bytes (int): The largest audio file a transcription takes, in bytes
            (`--max-audio-bytes`). A larger upload is refused with 413.
    """

    max_audio_bytes: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        max_audio_bytes = self.max_audio_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "max_audio_bytes": max_audio_bytes,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        max_audio_bytes = d.pop("max_audio_bytes")

        playground_config = cls(
            max_audio_bytes=max_audio_bytes,
        )

        playground_config.additional_properties = d
        return playground_config

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
