from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.playground_chat_answer_choices_item import (
        PlaygroundChatAnswerChoicesItem,
    )
    from ..models.playground_chat_answer_usage import PlaygroundChatAnswerUsage


T = TypeVar("T", bound="PlaygroundChatAnswer")


@_attrs_define
class PlaygroundChatAnswer:
    """The answer of `/v1/chat/completions`, in the OpenAI shape.

    Attributes:
        choices (list[PlaygroundChatAnswerChoicesItem]):
        created (int):
        id (str):
        model (str):
        object_ (str):
        usage (PlaygroundChatAnswerUsage):
    """

    choices: list[PlaygroundChatAnswerChoicesItem]
    created: int
    id: str
    model: str
    object_: str
    usage: PlaygroundChatAnswerUsage
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        choices = []
        for choices_item_data in self.choices:
            choices_item = choices_item_data.to_dict()
            choices.append(choices_item)

        created = self.created

        id = self.id

        model = self.model

        object_ = self.object_

        usage = self.usage.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "choices": choices,
                "created": created,
                "id": id,
                "model": model,
                "object": object_,
                "usage": usage,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.playground_chat_answer_choices_item import (
            PlaygroundChatAnswerChoicesItem,
        )
        from ..models.playground_chat_answer_usage import (
            PlaygroundChatAnswerUsage,
        )

        d = dict(src_dict)
        choices = []
        _choices = d.pop("choices")
        for choices_item_data in _choices:
            choices_item = PlaygroundChatAnswerChoicesItem.from_dict(choices_item_data)

            choices.append(choices_item)

        created = d.pop("created")

        id = d.pop("id")

        model = d.pop("model")

        object_ = d.pop("object")

        usage = PlaygroundChatAnswerUsage.from_dict(d.pop("usage"))

        playground_chat_answer = cls(
            choices=choices,
            created=created,
            id=id,
            model=model,
            object_=object_,
            usage=usage,
        )

        playground_chat_answer.additional_properties = d
        return playground_chat_answer

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
