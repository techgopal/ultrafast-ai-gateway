from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.params import Params
    from ..models.template_message import TemplateMessage


T = TypeVar("T", bound="PromptVersionEntry")


@_attrs_define
class PromptVersionEntry:
    """One version of a prompt template in a file.

    Attributes:
        messages (list[TemplateMessage]):
        version (int): From 1, in order, without gaps.
        model (None | str | Unset): Used when a call names no model.
        params (Params | Unset): The settings a version carries for the call. All optional; what the call
            itself sets wins.
    """

    messages: list[TemplateMessage]
    version: int
    model: None | str | Unset = UNSET
    params: Params | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        messages = []
        for messages_item_data in self.messages:
            messages_item = messages_item_data.to_dict()
            messages.append(messages_item)

        version = self.version

        model: None | str | Unset
        if isinstance(self.model, Unset):
            model = UNSET
        else:
            model = self.model

        params: dict[str, Any] | Unset = UNSET
        if not isinstance(self.params, Unset):
            params = self.params.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "messages": messages,
                "version": version,
            }
        )
        if model is not UNSET:
            field_dict["model"] = model
        if params is not UNSET:
            field_dict["params"] = params

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

        version = d.pop("version")

        def _parse_model(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model = _parse_model(d.pop("model", UNSET))

        _params = d.pop("params", UNSET)
        params: Params | Unset
        if isinstance(_params, Unset):
            params = UNSET
        else:
            params = Params.from_dict(_params)

        prompt_version_entry = cls(
            messages=messages,
            version=version,
            model=model,
            params=params,
        )

        return prompt_version_entry
