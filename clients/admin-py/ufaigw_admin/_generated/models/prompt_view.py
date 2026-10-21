from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.version_stub_view import VersionStubView


T = TypeVar("T", bound="PromptView")


@_attrs_define
class PromptView:
    """A template with the numbers of its versions, oldest first. The model and
    variables are those of the latest version; the text of any version is
    read from the version endpoint.

        Attributes:
            created_at (str):
            created_by (int | None):
            description (str):
            id (int):
            latest_version (int):
            model (None | str):
            name (str):
            unreadable (bool):
            updated_at (str):
            variables (list[str]):
            version_count (int):
            versions (list[VersionStubView]):
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
    versions: list[VersionStubView]
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

        versions = []
        for versions_item_data in self.versions:
            versions_item = versions_item_data.to_dict()
            versions.append(versions_item)

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
                "versions": versions,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.version_stub_view import VersionStubView

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

        versions = []
        _versions = d.pop("versions")
        for versions_item_data in _versions:
            versions_item = VersionStubView.from_dict(versions_item_data)

            versions.append(versions_item)

        prompt_view = cls(
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
            versions=versions,
        )

        prompt_view.additional_properties = d
        return prompt_view

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
