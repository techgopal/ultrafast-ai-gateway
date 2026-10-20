from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="CreateProviderRequest")


@_attrs_define
class CreateProviderRequest:
    """
    Attributes:
        base_url (str):
        kind (str):
        name (str):
        api_key (None | str | Unset):
        api_version (None | str | Unset): Azure OpenAI only; `2024-10-21` when left out.
    """

    base_url: str
    kind: str
    name: str
    api_key: None | str | Unset = UNSET
    api_version: None | str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        base_url = self.base_url

        kind = self.kind

        name = self.name

        api_key: None | str | Unset
        if isinstance(self.api_key, Unset):
            api_key = UNSET
        else:
            api_key = self.api_key

        api_version: None | str | Unset
        if isinstance(self.api_version, Unset):
            api_version = UNSET
        else:
            api_version = self.api_version

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "base_url": base_url,
                "kind": kind,
                "name": name,
            }
        )
        if api_key is not UNSET:
            field_dict["api_key"] = api_key
        if api_version is not UNSET:
            field_dict["api_version"] = api_version

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        base_url = d.pop("base_url")

        kind = d.pop("kind")

        name = d.pop("name")

        def _parse_api_key(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        api_key = _parse_api_key(d.pop("api_key", UNSET))

        def _parse_api_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        api_version = _parse_api_version(d.pop("api_version", UNSET))

        create_provider_request = cls(
            base_url=base_url,
            kind=kind,
            name=name,
            api_key=api_key,
            api_version=api_version,
        )

        return create_provider_request
