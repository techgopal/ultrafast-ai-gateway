from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..models.playground_chat_request_tool_choice_type_0 import (
    PlaygroundChatRequestToolChoiceType0,
)
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.playground_chat_request_prompt import PlaygroundChatRequestPrompt
    from ..models.playground_chat_request_response_format import (
        PlaygroundChatRequestResponseFormat,
    )
    from ..models.playground_chat_request_tool_choice_type_1 import (
        PlaygroundChatRequestToolChoiceType1,
    )
    from ..models.playground_chat_request_tools_item import (
        PlaygroundChatRequestToolsItem,
    )
    from ..models.playground_message import PlaygroundMessage


T = TypeVar("T", bound="PlaygroundChatRequest")


@_attrs_define
class PlaygroundChatRequest:
    """A chat request, as `/v1/chat/completions` takes it. The body is read by
    the same parser as that call's, so any field it accepts is accepted here.

        Attributes:
            max_tokens (int | Unset):
            messages (list[PlaygroundMessage] | Unset): Required unless `prompt` is set; with a template these come after
                its
                messages.
            model (str | Unset): A model as `provider/name`, or a route name. Required unless `prompt`
                is set and its template names a model.
            parallel_tool_calls (bool | Unset): As in `/v1/chat/completions`; ignored without `tools`.
            prompt (PlaygroundChatRequestPrompt | Unset): `id` is the template's name; `version` a positive integer, as a
                number or a string of digits (left out: the latest); `variables` maps each variable name to its text.
            response_format (PlaygroundChatRequestResponseFormat | Unset): As in `/v1/chat/completions`: `{"type":"text"}`,
                `{"type":"json_object"}` or `{"type":"json_schema","json_schema":{"name","schema","strict"?,"description"?}}`.
            stop (list[str] | Unset):
            stream (bool | Unset): Answer as server-sent events.
            temperature (float | Unset):
            tool_choice (PlaygroundChatRequestToolChoiceType0 | PlaygroundChatRequestToolChoiceType1 | Unset): `auto`,
                `none`, `required` or a named function, as in `/v1/chat/completions`. Without `tools`, `required` and a named
                function are refused with 400.
            tools (list[PlaygroundChatRequestToolsItem] | Unset): Functions the model may call, as in
                `/v1/chat/completions`.
            top_p (float | Unset):
    """

    max_tokens: int | Unset = UNSET
    messages: list[PlaygroundMessage] | Unset = UNSET
    model: str | Unset = UNSET
    parallel_tool_calls: bool | Unset = UNSET
    prompt: PlaygroundChatRequestPrompt | Unset = UNSET
    response_format: PlaygroundChatRequestResponseFormat | Unset = UNSET
    stop: list[str] | Unset = UNSET
    stream: bool | Unset = UNSET
    temperature: float | Unset = UNSET
    tool_choice: (
        PlaygroundChatRequestToolChoiceType0
        | PlaygroundChatRequestToolChoiceType1
        | Unset
    ) = UNSET
    tools: list[PlaygroundChatRequestToolsItem] | Unset = UNSET
    top_p: float | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        max_tokens = self.max_tokens

        messages: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.messages, Unset):
            messages = []
            for messages_item_data in self.messages:
                messages_item = messages_item_data.to_dict()
                messages.append(messages_item)

        model = self.model

        parallel_tool_calls = self.parallel_tool_calls

        prompt: dict[str, Any] | Unset = UNSET
        if not isinstance(self.prompt, Unset):
            prompt = self.prompt.to_dict()

        response_format: dict[str, Any] | Unset = UNSET
        if not isinstance(self.response_format, Unset):
            response_format = self.response_format.to_dict()

        stop: list[str] | Unset = UNSET
        if not isinstance(self.stop, Unset):
            stop = self.stop

        stream = self.stream

        temperature = self.temperature

        tool_choice: dict[str, Any] | str | Unset
        if isinstance(self.tool_choice, Unset):
            tool_choice = UNSET
        elif isinstance(self.tool_choice, PlaygroundChatRequestToolChoiceType0):
            tool_choice = self.tool_choice.value
        else:
            tool_choice = self.tool_choice.to_dict()

        tools: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.tools, Unset):
            tools = []
            for tools_item_data in self.tools:
                tools_item = tools_item_data.to_dict()
                tools.append(tools_item)

        top_p = self.top_p

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if max_tokens is not UNSET:
            field_dict["max_tokens"] = max_tokens
        if messages is not UNSET:
            field_dict["messages"] = messages
        if model is not UNSET:
            field_dict["model"] = model
        if parallel_tool_calls is not UNSET:
            field_dict["parallel_tool_calls"] = parallel_tool_calls
        if prompt is not UNSET:
            field_dict["prompt"] = prompt
        if response_format is not UNSET:
            field_dict["response_format"] = response_format
        if stop is not UNSET:
            field_dict["stop"] = stop
        if stream is not UNSET:
            field_dict["stream"] = stream
        if temperature is not UNSET:
            field_dict["temperature"] = temperature
        if tool_choice is not UNSET:
            field_dict["tool_choice"] = tool_choice
        if tools is not UNSET:
            field_dict["tools"] = tools
        if top_p is not UNSET:
            field_dict["top_p"] = top_p

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.playground_chat_request_prompt import (
            PlaygroundChatRequestPrompt,
        )
        from ..models.playground_chat_request_response_format import (
            PlaygroundChatRequestResponseFormat,
        )
        from ..models.playground_chat_request_tool_choice_type_1 import (
            PlaygroundChatRequestToolChoiceType1,
        )
        from ..models.playground_chat_request_tools_item import (
            PlaygroundChatRequestToolsItem,
        )
        from ..models.playground_message import PlaygroundMessage

        d = dict(src_dict)
        max_tokens = d.pop("max_tokens", UNSET)

        _messages = d.pop("messages", UNSET)
        messages: list[PlaygroundMessage] | Unset = UNSET
        if _messages is not UNSET:
            messages = []
            for messages_item_data in _messages:
                messages_item = PlaygroundMessage.from_dict(messages_item_data)

                messages.append(messages_item)

        model = d.pop("model", UNSET)

        parallel_tool_calls = d.pop("parallel_tool_calls", UNSET)

        _prompt = d.pop("prompt", UNSET)
        prompt: PlaygroundChatRequestPrompt | Unset
        if isinstance(_prompt, Unset):
            prompt = UNSET
        else:
            prompt = PlaygroundChatRequestPrompt.from_dict(_prompt)

        _response_format = d.pop("response_format", UNSET)
        response_format: PlaygroundChatRequestResponseFormat | Unset
        if isinstance(_response_format, Unset):
            response_format = UNSET
        else:
            response_format = PlaygroundChatRequestResponseFormat.from_dict(
                _response_format
            )

        stop = cast(list[str], d.pop("stop", UNSET))

        stream = d.pop("stream", UNSET)

        temperature = d.pop("temperature", UNSET)

        def _parse_tool_choice(
            data: object,
        ) -> (
            PlaygroundChatRequestToolChoiceType0
            | PlaygroundChatRequestToolChoiceType1
            | Unset
        ):
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                tool_choice_type_0 = PlaygroundChatRequestToolChoiceType0(data)

                return tool_choice_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            tool_choice_type_1 = PlaygroundChatRequestToolChoiceType1.from_dict(data)

            return tool_choice_type_1

        tool_choice = _parse_tool_choice(d.pop("tool_choice", UNSET))

        _tools = d.pop("tools", UNSET)
        tools: list[PlaygroundChatRequestToolsItem] | Unset = UNSET
        if _tools is not UNSET:
            tools = []
            for tools_item_data in _tools:
                tools_item = PlaygroundChatRequestToolsItem.from_dict(tools_item_data)

                tools.append(tools_item)

        top_p = d.pop("top_p", UNSET)

        playground_chat_request = cls(
            max_tokens=max_tokens,
            messages=messages,
            model=model,
            parallel_tool_calls=parallel_tool_calls,
            prompt=prompt,
            response_format=response_format,
            stop=stop,
            stream=stream,
            temperature=temperature,
            tool_choice=tool_choice,
            tools=tools,
            top_p=top_p,
        )

        playground_chat_request.additional_properties = d
        return playground_chat_request

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
