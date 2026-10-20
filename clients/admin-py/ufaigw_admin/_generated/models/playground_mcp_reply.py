from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.playground_mcp_reply_result import PlaygroundMcpReplyResult


T = TypeVar("T", bound="PlaygroundMcpReply")


@_attrs_define
class PlaygroundMcpReply:
    """The JSON-RPC answer of `/mcp/{server}` to a call that was carried out.
    (A call the tool could not do is an `isError` result; a call the gateway
    would not let through, such as one to a tool the user may not call, is a
    JSON-RPC `error` in the place of `result`, see `PlaygroundMcpError`.)

        Attributes:
            jsonrpc (str):
            result (PlaygroundMcpReplyResult):
            id (int | None | Unset): The `id` of the request.
    """

    jsonrpc: str
    result: PlaygroundMcpReplyResult
    id: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        jsonrpc = self.jsonrpc

        result = self.result.to_dict()

        id: int | None | Unset
        if isinstance(self.id, Unset):
            id = UNSET
        else:
            id = self.id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "jsonrpc": jsonrpc,
                "result": result,
            }
        )
        if id is not UNSET:
            field_dict["id"] = id

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.playground_mcp_reply_result import (
            PlaygroundMcpReplyResult,
        )

        d = dict(src_dict)
        jsonrpc = d.pop("jsonrpc")

        result = PlaygroundMcpReplyResult.from_dict(d.pop("result"))

        def _parse_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        id = _parse_id(d.pop("id", UNSET))

        playground_mcp_reply = cls(
            jsonrpc=jsonrpc,
            result=result,
            id=id,
        )

        playground_mcp_reply.additional_properties = d
        return playground_mcp_reply

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
