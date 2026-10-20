from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.prompt_version_entry import PromptVersionEntry


T = TypeVar("T", bound="PromptEntry")


@_attrs_define
class PromptEntry:
    """A prompt template with all its versions. An import adds the versions a
    gateway lacks and never rewrites one it has: a version that differs is
    an error. Who made a template is not in a file; an import makes the
    templates it creates the importing admin's.

        Attributes:
            name (str):
            versions (list[PromptVersionEntry]):
            description (str | Unset):
    """

    name: str
    versions: list[PromptVersionEntry]
    description: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        versions = []
        for versions_item_data in self.versions:
            versions_item = versions_item_data.to_dict()
            versions.append(versions_item)

        description = self.description

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "name": name,
                "versions": versions,
            }
        )
        if description is not UNSET:
            field_dict["description"] = description

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.prompt_version_entry import PromptVersionEntry

        d = dict(src_dict)
        name = d.pop("name")

        versions = []
        _versions = d.pop("versions")
        for versions_item_data in _versions:
            versions_item = PromptVersionEntry.from_dict(versions_item_data)

            versions.append(versions_item)

        description = d.pop("description", UNSET)

        prompt_entry = cls(
            name=name,
            versions=versions,
            description=description,
        )

        return prompt_entry
