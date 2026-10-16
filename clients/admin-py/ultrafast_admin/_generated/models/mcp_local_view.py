from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="McpLocalView")


@_attrs_define
class McpLocalView:
    """How the program of a local server stands.

    Attributes:
        phase (str): `starting`, `running`, `backoff` (died, waiting to be started again),
            `crashlooping`, `stopped`, or `off` (local servers are not enabled).
        restarts (int): How many times it was started again.
    """

    phase: str
    restarts: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        phase = self.phase

        restarts = self.restarts

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "phase": phase,
                "restarts": restarts,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        phase = d.pop("phase")

        restarts = d.pop("restarts")

        mcp_local_view = cls(
            phase=phase,
            restarts=restarts,
        )

        mcp_local_view.additional_properties = d
        return mcp_local_view

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
