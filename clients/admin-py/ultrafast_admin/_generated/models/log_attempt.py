from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..types import UNSET, Unset

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
        skipped (None | str | Unset): Why the target was passed over without a call, when the request could
            not be expressed for it: `unsupported:<feature>`. Absent otherwise.
    """

    duration_ms: int
    model: str
    outcome: str
    provider: str
    status: int | None
    skipped: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        duration_ms = self.duration_ms

        model = self.model

        outcome = self.outcome

        provider = self.provider

        status: int | None
        status = self.status

        skipped: None | str | Unset
        if isinstance(self.skipped, Unset):
            skipped = UNSET
        else:
            skipped = self.skipped

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
        if skipped is not UNSET:
            field_dict["skipped"] = skipped

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

        def _parse_skipped(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        skipped = _parse_skipped(d.pop("skipped", UNSET))

        log_attempt = cls(
            duration_ms=duration_ms,
            model=model,
            outcome=outcome,
            provider=provider,
            status=status,
            skipped=skipped,
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
