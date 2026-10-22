from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="UpdateModelRequest")


@_attrs_define
class UpdateModelRequest:
    """
    Attributes:
        cache_read_price_micros (int | None | Unset): Like `input_price_micros`, for tokens read from the prompt cache.
            `null` prices them at the input price.
        cache_write_price_micros (int | None | Unset): Like `cache_read_price_micros`, for tokens written to the prompt
            cache.
        enabled (bool | None | Unset): Left out, the model stays as it is.
        input_price_micros (int | None | Unset): Millionths of a dollar per million input tokens, 0 or more. Left
            out, the price stays; `null` makes it unknown.
        output_price_micros (int | None | Unset): Like `input_price_micros`, for output tokens.
    """

    cache_read_price_micros: int | None | Unset = UNSET
    cache_write_price_micros: int | None | Unset = UNSET
    enabled: bool | None | Unset = UNSET
    input_price_micros: int | None | Unset = UNSET
    output_price_micros: int | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        cache_read_price_micros: int | None | Unset
        if isinstance(self.cache_read_price_micros, Unset):
            cache_read_price_micros = UNSET
        else:
            cache_read_price_micros = self.cache_read_price_micros

        cache_write_price_micros: int | None | Unset
        if isinstance(self.cache_write_price_micros, Unset):
            cache_write_price_micros = UNSET
        else:
            cache_write_price_micros = self.cache_write_price_micros

        enabled: bool | None | Unset
        if isinstance(self.enabled, Unset):
            enabled = UNSET
        else:
            enabled = self.enabled

        input_price_micros: int | None | Unset
        if isinstance(self.input_price_micros, Unset):
            input_price_micros = UNSET
        else:
            input_price_micros = self.input_price_micros

        output_price_micros: int | None | Unset
        if isinstance(self.output_price_micros, Unset):
            output_price_micros = UNSET
        else:
            output_price_micros = self.output_price_micros

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if cache_read_price_micros is not UNSET:
            field_dict["cache_read_price_micros"] = cache_read_price_micros
        if cache_write_price_micros is not UNSET:
            field_dict["cache_write_price_micros"] = cache_write_price_micros
        if enabled is not UNSET:
            field_dict["enabled"] = enabled
        if input_price_micros is not UNSET:
            field_dict["input_price_micros"] = input_price_micros
        if output_price_micros is not UNSET:
            field_dict["output_price_micros"] = output_price_micros

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_cache_read_price_micros(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        cache_read_price_micros = _parse_cache_read_price_micros(
            d.pop("cache_read_price_micros", UNSET)
        )

        def _parse_cache_write_price_micros(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        cache_write_price_micros = _parse_cache_write_price_micros(
            d.pop("cache_write_price_micros", UNSET)
        )

        def _parse_enabled(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        enabled = _parse_enabled(d.pop("enabled", UNSET))

        def _parse_input_price_micros(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        input_price_micros = _parse_input_price_micros(
            d.pop("input_price_micros", UNSET)
        )

        def _parse_output_price_micros(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        output_price_micros = _parse_output_price_micros(
            d.pop("output_price_micros", UNSET)
        )

        update_model_request = cls(
            cache_read_price_micros=cache_read_price_micros,
            cache_write_price_micros=cache_write_price_micros,
            enabled=enabled,
            input_price_micros=input_price_micros,
            output_price_micros=output_price_micros,
        )

        return update_model_request
