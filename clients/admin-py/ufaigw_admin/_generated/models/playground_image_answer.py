from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.playground_image_answer_data_item import PlaygroundImageAnswerDataItem
    from ..models.playground_image_answer_usage import PlaygroundImageAnswerUsage


T = TypeVar("T", bound="PlaygroundImageAnswer")


@_attrs_define
class PlaygroundImageAnswer:
    """The answer of `/v1/images/generations`, in the OpenAI shape.

    Attributes:
        created (int):
        data (list[PlaygroundImageAnswerDataItem]): Each image as `b64_json` or `url`, with `revised_prompt` when the
            model gave one.
        usage (PlaygroundImageAnswerUsage | Unset): Only when the provider reports token usage.
    """

    created: int
    data: list[PlaygroundImageAnswerDataItem]
    usage: PlaygroundImageAnswerUsage | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        created = self.created

        data = []
        for data_item_data in self.data:
            data_item = data_item_data.to_dict()
            data.append(data_item)

        usage: dict[str, Any] | Unset = UNSET
        if not isinstance(self.usage, Unset):
            usage = self.usage.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "created": created,
                "data": data,
            }
        )
        if usage is not UNSET:
            field_dict["usage"] = usage

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.playground_image_answer_data_item import (
            PlaygroundImageAnswerDataItem,
        )
        from ..models.playground_image_answer_usage import (
            PlaygroundImageAnswerUsage,
        )

        d = dict(src_dict)
        created = d.pop("created")

        data = []
        _data = d.pop("data")
        for data_item_data in _data:
            data_item = PlaygroundImageAnswerDataItem.from_dict(data_item_data)

            data.append(data_item)

        _usage = d.pop("usage", UNSET)
        usage: PlaygroundImageAnswerUsage | Unset
        if isinstance(_usage, Unset):
            usage = UNSET
        else:
            usage = PlaygroundImageAnswerUsage.from_dict(_usage)

        playground_image_answer = cls(
            created=created,
            data=data,
            usage=usage,
        )

        playground_image_answer.additional_properties = d
        return playground_image_answer

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
