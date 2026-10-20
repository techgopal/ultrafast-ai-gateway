from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

T = TypeVar("T", bound="ProviderEntry")


@_attrs_define
class ProviderEntry:
    """
    Attributes:
        api_version (None | str): Azure OpenAI only.
        base_url (str):
        kind (str): `openai`, `anthropic`, `gemini` or `azure`.
        name (str):
    """

    api_version: None | str
    base_url: str
    kind: str
    name: str

    def to_dict(self) -> dict[str, Any]:
        api_version: None | str
        api_version = self.api_version

        base_url = self.base_url

        kind = self.kind

        name = self.name

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "api_version": api_version,
                "base_url": base_url,
                "kind": kind,
                "name": name,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_api_version(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        api_version = _parse_api_version(d.pop("api_version"))

        base_url = d.pop("base_url")

        kind = d.pop("kind")

        name = d.pop("name")

        provider_entry = cls(
            api_version=api_version,
            base_url=base_url,
            kind=kind,
            name=name,
        )

        return provider_entry
