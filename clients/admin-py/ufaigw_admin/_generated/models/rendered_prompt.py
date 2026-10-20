from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.params import Params
    from ..models.template_message import TemplateMessage


T = TypeVar("T", bound="RenderedPrompt")


@_attrs_define
class RenderedPrompt:
    """What a version renders to.

    Attributes:
        messages (list[TemplateMessage]): The messages with the values put in, as a call would get them.
        model (None | str): The model the template names, used when a call names none.
        params (Params): The settings a version carries for the call. All optional; what the call
            itself sets wins.
        version (int):
    """

    messages: list[TemplateMessage]
    model: None | str
    params: Params
    version: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        messages = []
        for messages_item_data in self.messages:
            messages_item = messages_item_data.to_dict()
            messages.append(messages_item)

        model: None | str
        model = self.model

        params = self.params.to_dict()

        version = self.version

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "messages": messages,
                "model": model,
                "params": params,
                "version": version,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.params import Params
        from ..models.template_message import TemplateMessage

        d = dict(src_dict)
        messages = []
        _messages = d.pop("messages")
        for messages_item_data in _messages:
            messages_item = TemplateMessage.from_dict(messages_item_data)

            messages.append(messages_item)

        def _parse_model(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        model = _parse_model(d.pop("model"))

        params = Params.from_dict(d.pop("params"))

        version = d.pop("version")

        rendered_prompt = cls(
            messages=messages,
            model=model,
            params=params,
            version=version,
        )

        rendered_prompt.additional_properties = d
        return rendered_prompt

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
