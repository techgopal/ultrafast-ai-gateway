from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.params_response_format_type_0 import ParamsResponseFormatType0


T = TypeVar("T", bound="Params")


@_attrs_define
class Params:
    """The settings a version carries for the call. All optional; what the call
    itself sets wins.

        Attributes:
            max_tokens (int | None | Unset): At least 1.
            response_format (None | ParamsResponseFormatType0 | Unset): As in `/v1/chat/completions`: `{"type":"text"}`,
                `{"type":"json_object"}`
                or `{"type":"json_schema","json_schema":{...}}`.
            temperature (float | None | Unset): 0 to 2.
            top_p (float | None | Unset): 0 to 1.
    """

    max_tokens: int | None | Unset = UNSET
    response_format: None | ParamsResponseFormatType0 | Unset = UNSET
    temperature: float | None | Unset = UNSET
    top_p: float | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        from ..models.params_response_format_type_0 import (
            ParamsResponseFormatType0,
        )

        max_tokens: int | None | Unset
        if isinstance(self.max_tokens, Unset):
            max_tokens = UNSET
        else:
            max_tokens = self.max_tokens

        response_format: dict[str, Any] | None | Unset
        if isinstance(self.response_format, Unset):
            response_format = UNSET
        elif isinstance(self.response_format, ParamsResponseFormatType0):
            response_format = self.response_format.to_dict()
        else:
            response_format = self.response_format

        temperature: float | None | Unset
        if isinstance(self.temperature, Unset):
            temperature = UNSET
        else:
            temperature = self.temperature

        top_p: float | None | Unset
        if isinstance(self.top_p, Unset):
            top_p = UNSET
        else:
            top_p = self.top_p

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if max_tokens is not UNSET:
            field_dict["max_tokens"] = max_tokens
        if response_format is not UNSET:
            field_dict["response_format"] = response_format
        if temperature is not UNSET:
            field_dict["temperature"] = temperature
        if top_p is not UNSET:
            field_dict["top_p"] = top_p

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.params_response_format_type_0 import (
            ParamsResponseFormatType0,
        )

        d = dict(src_dict)

        def _parse_max_tokens(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        max_tokens = _parse_max_tokens(d.pop("max_tokens", UNSET))

        def _parse_response_format(
            data: object,
        ) -> None | ParamsResponseFormatType0 | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                response_format_type_0 = ParamsResponseFormatType0.from_dict(data)

                return response_format_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | ParamsResponseFormatType0 | Unset, data)

        response_format = _parse_response_format(d.pop("response_format", UNSET))

        def _parse_temperature(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        temperature = _parse_temperature(d.pop("temperature", UNSET))

        def _parse_top_p(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        top_p = _parse_top_p(d.pop("top_p", UNSET))

        params = cls(
            max_tokens=max_tokens,
            response_format=response_format,
            temperature=temperature,
            top_p=top_p,
        )

        return params
