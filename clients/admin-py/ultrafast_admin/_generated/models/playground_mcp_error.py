from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.playground_mcp_error_error import PlaygroundMcpErrorError


T = TypeVar("T", bound="PlaygroundMcpError")


@_attrs_define
class PlaygroundMcpError:
    """The JSON-RPC answer of `/mcp/{server}` to a request it refused.

    Attributes:
        error (PlaygroundMcpErrorError): `{"code": <number>, "message": <text>}`.
        jsonrpc (str):
        id (int | None | Unset): The `id` of the request; `null` when it could not be read.
    """

    error: PlaygroundMcpErrorError
    jsonrpc: str
    id: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        error = self.error.to_dict()

        jsonrpc = self.jsonrpc

        id: int | None | Unset
        if isinstance(self.id, Unset):
            id = UNSET
        else:
            id = self.id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "error": error,
                "jsonrpc": jsonrpc,
            }
        )
        if id is not UNSET:
            field_dict["id"] = id

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.playground_mcp_error_error import (
            PlaygroundMcpErrorError,
        )

        d = dict(src_dict)
        error = PlaygroundMcpErrorError.from_dict(d.pop("error"))

        jsonrpc = d.pop("jsonrpc")

        def _parse_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        id = _parse_id(d.pop("id", UNSET))

        playground_mcp_error = cls(
            error=error,
            jsonrpc=jsonrpc,
            id=id,
        )

        playground_mcp_error.additional_properties = d
        return playground_mcp_error

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
