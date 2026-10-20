from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.playground_chat_request_prompt_variables import (
        PlaygroundChatRequestPromptVariables,
    )


T = TypeVar("T", bound="PlaygroundChatRequestPrompt")


@_attrs_define
class PlaygroundChatRequestPrompt:
    """`id` is the template's name; `version` a positive integer, as a number or a string of digits (left out: the latest);
    `variables` maps each variable name to its text.

        Attributes:
            id (str):
            variables (PlaygroundChatRequestPromptVariables | Unset):
            version (int | Unset): A positive integer. A string of digits is read as the same number.
    """

    id: str
    variables: PlaygroundChatRequestPromptVariables | Unset = UNSET
    version: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        variables: dict[str, Any] | Unset = UNSET
        if not isinstance(self.variables, Unset):
            variables = self.variables.to_dict()

        version = self.version

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
            }
        )
        if variables is not UNSET:
            field_dict["variables"] = variables
        if version is not UNSET:
            field_dict["version"] = version

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.playground_chat_request_prompt_variables import (
            PlaygroundChatRequestPromptVariables,
        )

        d = dict(src_dict)
        id = d.pop("id")

        _variables = d.pop("variables", UNSET)
        variables: PlaygroundChatRequestPromptVariables | Unset
        if isinstance(_variables, Unset):
            variables = UNSET
        else:
            variables = PlaygroundChatRequestPromptVariables.from_dict(_variables)

        version = d.pop("version", UNSET)

        playground_chat_request_prompt = cls(
            id=id,
            variables=variables,
            version=version,
        )

        playground_chat_request_prompt.additional_properties = d
        return playground_chat_request_prompt

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
