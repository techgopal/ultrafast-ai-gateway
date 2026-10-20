from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="McpHealthView")


@_attrs_define
class McpHealthView:
    """The finding of a health probe: one `tools/list` outside any key, with a
    5 second limit, every `UF_MCP_HEALTH_INTERVAL` seconds (60).

        Attributes:
            checked_at (str):
            latency_ms (int | None): How long the answer took, when there was one.
            status (str): `ok`, `degraded` (slow, or one failed probe), `down` (two failed probes
                in a row) or `crashlooping` (a local program that keeps dying).
    """

    checked_at: str
    latency_ms: int | None
    status: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        checked_at = self.checked_at

        latency_ms: int | None
        latency_ms = self.latency_ms

        status = self.status

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "checked_at": checked_at,
                "latency_ms": latency_ms,
                "status": status,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        checked_at = d.pop("checked_at")

        def _parse_latency_ms(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        latency_ms = _parse_latency_ms(d.pop("latency_ms"))

        status = d.pop("status")

        mcp_health_view = cls(
            checked_at=checked_at,
            latency_ms=latency_ms,
            status=status,
        )

        mcp_health_view.additional_properties = d
        return mcp_health_view

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
