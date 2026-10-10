from __future__ import annotations

from collections.abc import Mapping
from io import BytesIO
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from .. import types
from ..types import UNSET, File, Unset

T = TypeVar("T", bound="PlaygroundTranscriptionForm")


@_attrs_define
class PlaygroundTranscriptionForm:
    """The form of a transcription, as `/v1/audio/transcriptions` takes it
    (`multipart/form-data`). The form is read by the same reader as that
    call's: the file may not be larger than the gateway's audio cap
    (`UF_MAX_AUDIO_BYTES`, 25 MiB by default).

        Attributes:
            file (File): The audio file.
            model (str): A model as `provider/name`, or a route name.
            language (str | Unset): The language spoken, as an ISO-639-1 code.
            prompt (str | Unset): Text to guide the style of the transcript.
            response_format (str | Unset): `json`, `text`, `verbose_json`, `srt` or `vtt`.
            temperature (float | Unset): From 0 to 1.
    """

    file: File
    model: str
    language: str | Unset = UNSET
    prompt: str | Unset = UNSET
    response_format: str | Unset = UNSET
    temperature: float | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        file = self.file.to_tuple()

        model = self.model

        language = self.language

        prompt = self.prompt

        response_format = self.response_format

        temperature = self.temperature

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "file": file,
                "model": model,
            }
        )
        if language is not UNSET:
            field_dict["language"] = language
        if prompt is not UNSET:
            field_dict["prompt"] = prompt
        if response_format is not UNSET:
            field_dict["response_format"] = response_format
        if temperature is not UNSET:
            field_dict["temperature"] = temperature

        return field_dict

    def to_multipart(self) -> types.RequestFiles:
        files: types.RequestFiles = []

        files.append(("file", self.file.to_tuple()))

        files.append(("model", (None, str(self.model).encode(), "text/plain")))

        if not isinstance(self.language, Unset):
            files.append(
                ("language", (None, str(self.language).encode(), "text/plain"))
            )

        if not isinstance(self.prompt, Unset):
            files.append(("prompt", (None, str(self.prompt).encode(), "text/plain")))

        if not isinstance(self.response_format, Unset):
            files.append(
                (
                    "response_format",
                    (None, str(self.response_format).encode(), "text/plain"),
                )
            )

        if not isinstance(self.temperature, Unset):
            files.append(
                ("temperature", (None, str(self.temperature).encode(), "text/plain"))
            )

        for prop_name, prop in self.additional_properties.items():
            files.append((prop_name, (None, str(prop).encode(), "text/plain")))

        return files

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        file = File(payload=BytesIO(d.pop("file")))

        model = d.pop("model")

        language = d.pop("language", UNSET)

        prompt = d.pop("prompt", UNSET)

        response_format = d.pop("response_format", UNSET)

        temperature = d.pop("temperature", UNSET)

        playground_transcription_form = cls(
            file=file,
            model=model,
            language=language,
            prompt=prompt,
            response_format=response_format,
            temperature=temperature,
        )

        playground_transcription_form.additional_properties = d
        return playground_transcription_form

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
