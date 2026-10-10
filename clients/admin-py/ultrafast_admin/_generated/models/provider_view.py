from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="ProviderView")


@_attrs_define
class ProviderView:
    """A provider as `/api` shows it: whether it has a credential, never the
    credential.

        Attributes:
            base_url (None | str): Where the provider is called. Admins only: `null` for anybody else.
            has_credential (bool):
            id (int):
            kind (str):
            name (str):
            api_version (None | str | Unset): Set for Azure OpenAI providers.
    """

    base_url: None | str
    has_credential: bool
    id: int
    kind: str
    name: str
    api_version: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        base_url: None | str
        base_url = self.base_url

        has_credential = self.has_credential

        id = self.id

        kind = self.kind

        name = self.name

        api_version: None | str | Unset
        if isinstance(self.api_version, Unset):
            api_version = UNSET
        else:
            api_version = self.api_version

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "base_url": base_url,
                "has_credential": has_credential,
                "id": id,
                "kind": kind,
                "name": name,
            }
        )
        if api_version is not UNSET:
            field_dict["api_version"] = api_version

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_base_url(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        base_url = _parse_base_url(d.pop("base_url"))

        has_credential = d.pop("has_credential")

        id = d.pop("id")

        kind = d.pop("kind")

        name = d.pop("name")

        def _parse_api_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        api_version = _parse_api_version(d.pop("api_version", UNSET))

        provider_view = cls(
            base_url=base_url,
            has_credential=has_credential,
            id=id,
            kind=kind,
            name=name,
            api_version=api_version,
        )

        provider_view.additional_properties = d
        return provider_view

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
