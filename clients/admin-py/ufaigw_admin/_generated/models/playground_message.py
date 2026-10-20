from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.playground_message_content_type_1_item import (
        PlaygroundMessageContentType1Item,
    )
    from ..models.playground_message_tool_calls_item import (
        PlaygroundMessageToolCallsItem,
    )


T = TypeVar("T", bound="PlaygroundMessage")


@_attrs_define
class PlaygroundMessage:
    """
    Attributes:
        role (str): `system`, `user`, `assistant` or `tool`.
        content (list[PlaygroundMessageContentType1Item] | None | str | Unset): Text, or a list of parts (`text` and
            `image_url`) as in `/v1/chat/completions`. Null is allowed on an assistant message that has `tool_calls`. Images
            are `data:` URLs or, except for Gemini, `http(s)` URLs, and count toward the request body limit (10 MiB).
        tool_call_id (str | Unset): On a `tool` message: the id of the call it answers.
        tool_calls (list[PlaygroundMessageToolCallsItem] | Unset): The calls of an assistant message, as in
            `/v1/chat/completions`.
    """

    role: str
    content: list[PlaygroundMessageContentType1Item] | None | str | Unset = UNSET
    tool_call_id: str | Unset = UNSET
    tool_calls: list[PlaygroundMessageToolCallsItem] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        role = self.role

        content: list[dict[str, Any]] | None | str | Unset
        if isinstance(self.content, Unset):
            content = UNSET
        elif isinstance(self.content, list):
            content = []
            for content_type_1_item_data in self.content:
                content_type_1_item = content_type_1_item_data.to_dict()
                content.append(content_type_1_item)

        else:
            content = self.content

        tool_call_id = self.tool_call_id

        tool_calls: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.tool_calls, Unset):
            tool_calls = []
            for tool_calls_item_data in self.tool_calls:
                tool_calls_item = tool_calls_item_data.to_dict()
                tool_calls.append(tool_calls_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "role": role,
            }
        )
        if content is not UNSET:
            field_dict["content"] = content
        if tool_call_id is not UNSET:
            field_dict["tool_call_id"] = tool_call_id
        if tool_calls is not UNSET:
            field_dict["tool_calls"] = tool_calls

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.playground_message_content_type_1_item import (
            PlaygroundMessageContentType1Item,
        )
        from ..models.playground_message_tool_calls_item import (
            PlaygroundMessageToolCallsItem,
        )

        d = dict(src_dict)
        role = d.pop("role")

        def _parse_content(
            data: object,
        ) -> list[PlaygroundMessageContentType1Item] | None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                content_type_1 = []
                _content_type_1 = data
                for content_type_1_item_data in _content_type_1:
                    content_type_1_item = PlaygroundMessageContentType1Item.from_dict(
                        content_type_1_item_data
                    )

                    content_type_1.append(content_type_1_item)

                return content_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(
                list[PlaygroundMessageContentType1Item] | None | str | Unset, data
            )

        content = _parse_content(d.pop("content", UNSET))

        tool_call_id = d.pop("tool_call_id", UNSET)

        _tool_calls = d.pop("tool_calls", UNSET)
        tool_calls: list[PlaygroundMessageToolCallsItem] | Unset = UNSET
        if _tool_calls is not UNSET:
            tool_calls = []
            for tool_calls_item_data in _tool_calls:
                tool_calls_item = PlaygroundMessageToolCallsItem.from_dict(
                    tool_calls_item_data
                )

                tool_calls.append(tool_calls_item)

        playground_message = cls(
            role=role,
            content=content,
            tool_call_id=tool_call_id,
            tool_calls=tool_calls,
        )

        playground_message.additional_properties = d
        return playground_message

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
