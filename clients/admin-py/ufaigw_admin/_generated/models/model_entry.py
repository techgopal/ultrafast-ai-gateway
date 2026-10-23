from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.grant_entry import GrantEntry


T = TypeVar("T", bound="ModelEntry")


@_attrs_define
class ModelEntry:
    """
    Attributes:
        cache_read_price_micros (int | None): Per million tokens read from or written to the provider's prompt
            cache. `null`: the input price is used.
        cache_write_price_micros (int | None):
        enabled (bool):
        input_price_micros (int | None): Per million tokens, in millionths of a dollar. `null`: not known.
        name (str):
        output_price_micros (int | None):
        provider (str): The name of its provider.
        grants (GrantEntry | Unset):
    """

    cache_read_price_micros: int | None
    cache_write_price_micros: int | None
    enabled: bool
    input_price_micros: int | None
    name: str
    output_price_micros: int | None
    provider: str
    grants: GrantEntry | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        cache_read_price_micros: int | None
        cache_read_price_micros = self.cache_read_price_micros

        cache_write_price_micros: int | None
        cache_write_price_micros = self.cache_write_price_micros

        enabled = self.enabled

        input_price_micros: int | None
        input_price_micros = self.input_price_micros

        name = self.name

        output_price_micros: int | None
        output_price_micros = self.output_price_micros

        provider = self.provider

        grants: dict[str, Any] | Unset = UNSET
        if not isinstance(self.grants, Unset):
            grants = self.grants.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "cache_read_price_micros": cache_read_price_micros,
                "cache_write_price_micros": cache_write_price_micros,
                "enabled": enabled,
                "input_price_micros": input_price_micros,
                "name": name,
                "output_price_micros": output_price_micros,
                "provider": provider,
            }
        )
        if grants is not UNSET:
            field_dict["grants"] = grants

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.grant_entry import GrantEntry

        d = dict(src_dict)

        def _parse_cache_read_price_micros(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        cache_read_price_micros = _parse_cache_read_price_micros(
            d.pop("cache_read_price_micros")
        )

        def _parse_cache_write_price_micros(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        cache_write_price_micros = _parse_cache_write_price_micros(
            d.pop("cache_write_price_micros")
        )

        enabled = d.pop("enabled")

        def _parse_input_price_micros(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        input_price_micros = _parse_input_price_micros(d.pop("input_price_micros"))

        name = d.pop("name")

        def _parse_output_price_micros(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        output_price_micros = _parse_output_price_micros(d.pop("output_price_micros"))

        provider = d.pop("provider")

        _grants = d.pop("grants", UNSET)
        grants: GrantEntry | Unset
        if isinstance(_grants, Unset):
            grants = UNSET
        else:
            grants = GrantEntry.from_dict(_grants)

        model_entry = cls(
            cache_read_price_micros=cache_read_price_micros,
            cache_write_price_micros=cache_write_price_micros,
            enabled=enabled,
            input_price_micros=input_price_micros,
            name=name,
            output_price_micros=output_price_micros,
            provider=provider,
            grants=grants,
        )

        return model_entry
