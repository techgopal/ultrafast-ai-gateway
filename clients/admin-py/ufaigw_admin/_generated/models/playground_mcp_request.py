from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.playground_mcp_request_params import PlaygroundMcpRequestParams


T = TypeVar("T", bound="PlaygroundMcpRequest")


@_attrs_define
class PlaygroundMcpRequest:
    """A JSON-RPC request for `/mcp/{server}`, as the console's playground sends
    it: `tools/list` or `tools/call`. The body is read by the same dispatcher
    as that endpoint's.

        Attributes:
            id (int):
            jsonrpc (str): `2.0`.
            method (str): For example `tools/list`, or `tools/call` with `params`
                `{"name": "<tool>", "arguments": {...}}`.
            params (PlaygroundMcpRequestParams | Unset):
    """

    id: int
    jsonrpc: str
    method: str
    params: PlaygroundMcpRequestParams | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        jsonrpc = self.jsonrpc

        method = self.method

        params: dict[str, Any] | Unset = UNSET
        if not isinstance(self.params, Unset):
            params = self.params.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "jsonrpc": jsonrpc,
                "method": method,
            }
        )
        if params is not UNSET:
            field_dict["params"] = params

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.playground_mcp_request_params import (
            PlaygroundMcpRequestParams,
        )

        d = dict(src_dict)
        id = d.pop("id")

        jsonrpc = d.pop("jsonrpc")

        method = d.pop("method")

        _params = d.pop("params", UNSET)
        params: PlaygroundMcpRequestParams | Unset
        if isinstance(_params, Unset):
            params = UNSET
        else:
            params = PlaygroundMcpRequestParams.from_dict(_params)

        playground_mcp_request = cls(
            id=id,
            jsonrpc=jsonrpc,
            method=method,
            params=params,
        )

        playground_mcp_request.additional_properties = d
        return playground_mcp_request

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
