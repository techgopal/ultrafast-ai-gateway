from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="KeywordsMatcher")


@_attrs_define
class KeywordsMatcher:
    """
    Attributes:
        words (list[str]): Up to 1 000, each 1 to 256 characters. Case-insensitive.
        whole_word (bool | Unset): Match whole words only (the default); `false` matches substrings.
    """

    words: list[str]
    whole_word: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        words = self.words

        whole_word = self.whole_word

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "words": words,
            }
        )
        if whole_word is not UNSET:
            field_dict["whole_word"] = whole_word

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        words = cast(list[str], d.pop("words"))

        whole_word = d.pop("whole_word", UNSET)

        keywords_matcher = cls(
            words=words,
            whole_word=whole_word,
        )

        keywords_matcher.additional_properties = d
        return keywords_matcher

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
