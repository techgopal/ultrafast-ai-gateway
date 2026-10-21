from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.from_catalog_request_secrets import FromCatalogRequestSecrets


T = TypeVar("T", bound="FromCatalogRequest")


@_attrs_define
class FromCatalogRequest:
    """
    Attributes:
        catalog_id (str): The `id` of a catalog entry.
        name (None | str | Unset): The name of the server. The entry's id by default.
        secrets (FromCatalogRequestSecrets | Unset): The values of the entry's `env` items, by name. A secret item is
            required; the value of the one an HTTP entry authenticates with is
            the server's secret.
    """

    catalog_id: str
    name: None | str | Unset = UNSET
    secrets: FromCatalogRequestSecrets | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        catalog_id = self.catalog_id

        name: None | str | Unset
        if isinstance(self.name, Unset):
            name = UNSET
        else:
            name = self.name

        secrets: dict[str, Any] | Unset = UNSET
        if not isinstance(self.secrets, Unset):
            secrets = self.secrets.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "catalog_id": catalog_id,
            }
        )
        if name is not UNSET:
            field_dict["name"] = name
        if secrets is not UNSET:
            field_dict["secrets"] = secrets

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.from_catalog_request_secrets import (
            FromCatalogRequestSecrets,
        )

        d = dict(src_dict)
        catalog_id = d.pop("catalog_id")

        def _parse_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        name = _parse_name(d.pop("name", UNSET))

        _secrets = d.pop("secrets", UNSET)
        secrets: FromCatalogRequestSecrets | Unset
        if isinstance(_secrets, Unset):
            secrets = UNSET
        else:
            secrets = FromCatalogRequestSecrets.from_dict(_secrets)

        from_catalog_request = cls(
            catalog_id=catalog_id,
            name=name,
            secrets=secrets,
        )

        return from_catalog_request
