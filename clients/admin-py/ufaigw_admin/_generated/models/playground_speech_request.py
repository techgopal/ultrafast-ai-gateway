from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="PlaygroundSpeechRequest")


@_attrs_define
class PlaygroundSpeechRequest:
    """A speech request, as `/v1/audio/speech` takes it.

    Attributes:
        input_ (str): The text to speak, at most 4096 characters.
        model (str): A model as `provider/name`, or a route name.
        voice (str): A voice name such as `alloy`.
        instructions (str | Unset): How the text should be spoken (not for `tts-1` models).
        response_format (str | Unset): `mp3`, `opus`, `aac`, `flac`, `wav` or `pcm`.
        speed (float | Unset): From 0.25 to 4.0.
    """

    input_: str
    model: str
    voice: str
    instructions: str | Unset = UNSET
    response_format: str | Unset = UNSET
    speed: float | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        input_ = self.input_

        model = self.model

        voice = self.voice

        instructions = self.instructions

        response_format = self.response_format

        speed = self.speed

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "input": input_,
                "model": model,
                "voice": voice,
            }
        )
        if instructions is not UNSET:
            field_dict["instructions"] = instructions
        if response_format is not UNSET:
            field_dict["response_format"] = response_format
        if speed is not UNSET:
            field_dict["speed"] = speed

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        input_ = d.pop("input")

        model = d.pop("model")

        voice = d.pop("voice")

        instructions = d.pop("instructions", UNSET)

        response_format = d.pop("response_format", UNSET)

        speed = d.pop("speed", UNSET)

        playground_speech_request = cls(
            input_=input_,
            model=model,
            voice=voice,
            instructions=instructions,
            response_format=response_format,
            speed=speed,
        )

        playground_speech_request.additional_properties = d
        return playground_speech_request

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
