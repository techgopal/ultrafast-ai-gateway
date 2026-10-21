from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="PromptSummary")


@_attrs_define
class PromptSummary:
    """A template in a list.

    Attributes:
        created_at (str):
        created_by (int | None): The user who made it; `null` when they are gone. Admins manage
            every template, a lead the ones they made.
        description (str):
        id (int):
        latest_version (int): The version a call without `version` gets.
        model (None | str): The model and variables of the latest version.
        name (str):
        unreadable (bool): The latest version cannot be read, so `model` and `variables` are
            empty and a call by this name is refused. An admin can add a version.
        updated_at (str): When the latest version was written.
        variables (list[str]):
        version_count (int):
    """

    created_at: str
    created_by: int | None
    description: str
    id: int
    latest_version: int
    model: None | str
    name: str
    unreadable: bool
    updated_at: str
    variables: list[str]
    version_count: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        created_at = self.created_at

        created_by: int | None
        created_by = self.created_by

        description = self.description

        id = self.id

        latest_version = self.latest_version

        model: None | str
        model = self.model

        name = self.name

        unreadable = self.unreadable

        updated_at = self.updated_at

        variables = self.variables

        version_count = self.version_count

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "created_at": created_at,
                "created_by": created_by,
                "description": description,
                "id": id,
                "latest_version": latest_version,
                "model": model,
                "name": name,
                "unreadable": unreadable,
                "updated_at": updated_at,
                "variables": variables,
                "version_count": version_count,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        created_at = d.pop("created_at")

        def _parse_created_by(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        created_by = _parse_created_by(d.pop("created_by"))

        description = d.pop("description")

        id = d.pop("id")

        latest_version = d.pop("latest_version")

        def _parse_model(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        model = _parse_model(d.pop("model"))

        name = d.pop("name")

        unreadable = d.pop("unreadable")

        updated_at = d.pop("updated_at")

        variables = cast(list[str], d.pop("variables"))

        version_count = d.pop("version_count")

        prompt_summary = cls(
            created_at=created_at,
            created_by=created_by,
            description=description,
            id=id,
            latest_version=latest_version,
            model=model,
            name=name,
            unreadable=unreadable,
            updated_at=updated_at,
            variables=variables,
            version_count=version_count,
        )

        prompt_summary.additional_properties = d
        return prompt_summary

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
