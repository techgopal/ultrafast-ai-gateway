from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="PlaygroundImageRequest")


@_attrs_define
class PlaygroundImageRequest:
    """An image generation request, as `/v1/images/generations` takes it. The body
    is read by the same parser as that call's, so any field it accepts is
    accepted here.

        Attributes:
            model (str): A model as `provider/name`, or a route name.
            prompt (str):
            background (str | Unset): `transparent`, `opaque` or `auto`.
            n (int | Unset): The number of images, 1 to 10.
            output_format (str | Unset): `png`, `jpeg` or `webp`.
            quality (str | Unset):
            size (str | Unset): For example `1024x1024`.
    """

    model: str
    prompt: str
    background: str | Unset = UNSET
    n: int | Unset = UNSET
    output_format: str | Unset = UNSET
    quality: str | Unset = UNSET
    size: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        model = self.model

        prompt = self.prompt

        background = self.background

        n = self.n

        output_format = self.output_format

        quality = self.quality

        size = self.size

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "model": model,
                "prompt": prompt,
            }
        )
        if background is not UNSET:
            field_dict["background"] = background
        if n is not UNSET:
            field_dict["n"] = n
        if output_format is not UNSET:
            field_dict["output_format"] = output_format
        if quality is not UNSET:
            field_dict["quality"] = quality
        if size is not UNSET:
            field_dict["size"] = size

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        model = d.pop("model")

        prompt = d.pop("prompt")

        background = d.pop("background", UNSET)

        n = d.pop("n", UNSET)

        output_format = d.pop("output_format", UNSET)

        quality = d.pop("quality", UNSET)

        size = d.pop("size", UNSET)

        playground_image_request = cls(
            model=model,
            prompt=prompt,
            background=background,
            n=n,
            output_format=output_format,
            quality=quality,
            size=size,
        )

        playground_image_request.additional_properties = d
        return playground_image_request

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
