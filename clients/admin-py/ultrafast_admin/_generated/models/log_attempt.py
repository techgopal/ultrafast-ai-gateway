from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="LogAttempt")


@_attrs_define
class LogAttempt:
    """One target tried for a call.

    Attributes:
        duration_ms (int):
        model (str):
        outcome (str): `ok`, `retryable`, `fatal`, `circuit_open`, `skipped` or `cached` (answered from the response
            cache, no provider called).
        provider (str):
        status (int | None): What the provider answered, when it did.
    """

    duration_ms: int
    model: str
    outcome: str
    provider: str
    status: int | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        duration_ms = self.duration_ms

        model = self.model

        outcome = self.outcome

        provider = self.provider

        status: int | None
        status = self.status

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "duration_ms": duration_ms,
                "model": model,
                "outcome": outcome,
                "provider": provider,
                "status": status,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        duration_ms = d.pop("duration_ms")

        model = d.pop("model")

        outcome = d.pop("outcome")

        provider = d.pop("provider")

        def _parse_status(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        status = _parse_status(d.pop("status"))

        log_attempt = cls(
            duration_ms=duration_ms,
            model=model,
            outcome=outcome,
            provider=provider,
            status=status,
        )

        log_attempt.additional_properties = d
        return log_attempt

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
