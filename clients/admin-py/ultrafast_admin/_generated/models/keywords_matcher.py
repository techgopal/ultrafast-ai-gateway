from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="KeywordsMatcher")


@_attrs_define
class KeywordsMatcher:
    """The words of a keyword rule (a named type so that generated clients get a
    name for it).

        Attributes:
            words (list[str]): Up to 1 000, each 1 to 256 characters. Case-insensitive.
            whole_word (bool | Unset): Match whole words only (the default); `false` matches substrings.
    """

    words: list[str]
    whole_word: bool | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        words = self.words

        whole_word = self.whole_word

        field_dict: dict[str, Any] = {}

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

        return keywords_matcher
