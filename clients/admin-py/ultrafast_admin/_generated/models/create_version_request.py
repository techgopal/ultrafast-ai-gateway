from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.params import Params
    from ..models.template_message import TemplateMessage


T = TypeVar("T", bound="CreateVersionRequest")


@_attrs_define
class CreateVersionRequest:
    """
    Attributes:
        messages (list[TemplateMessage]): As for a new template. A version stands alone: it keeps nothing of
            the one before.
        model (None | str | Unset):
        params (None | Params | Unset):
    """

    messages: list[TemplateMessage]
    model: None | str | Unset = UNSET
    params: None | Params | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        from ..models.params import Params

        messages = []
        for messages_item_data in self.messages:
            messages_item = messages_item_data.to_dict()
            messages.append(messages_item)

        model: None | str | Unset
        if isinstance(self.model, Unset):
            model = UNSET
        else:
            model = self.model

        params: dict[str, Any] | None | Unset
        if isinstance(self.params, Unset):
            params = UNSET
        elif isinstance(self.params, Params):
            params = self.params.to_dict()
        else:
            params = self.params

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "messages": messages,
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

        def _parse_model(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model = _parse_model(d.pop("model", UNSET))

        def _parse_params(data: object) -> None | Params | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                params_type_0 = Params.from_dict(data)

                return params_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Params | Unset, data)

        params = _parse_params(d.pop("params", UNSET))

        create_version_request = cls(
            messages=messages,
            model=model,
            params=params,
        )

        return create_version_request
