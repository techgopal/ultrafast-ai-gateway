from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..models.target_state import TargetState

T = TypeVar("T", bound="TargetHealth")


@_attrs_define
class TargetHealth:
    """The health of one target, as `GET /api/routing/health` shows it.

    Attributes:
        failures (int): Retryable failures. A request the provider rejected is not one.
        last_failure_at (None | str): When the last of those happened (UTC, `YYYY-MM-DD HH:MM:SS`).
        last_status (int | None): What the provider answered to the last of them; none when it did not
            answer.
        model (str):
        provider (str):
        state (TargetState): What the breaker of a target allows now.
        successes (int):
    """

    failures: int
    last_failure_at: None | str
    last_status: int | None
    model: str
    provider: str
    state: TargetState
    successes: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        failures = self.failures

        last_failure_at: None | str
        last_failure_at = self.last_failure_at

        last_status: int | None
        last_status = self.last_status

        model = self.model

        provider = self.provider

        state = self.state.value

        successes = self.successes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "failures": failures,
                "last_failure_at": last_failure_at,
                "last_status": last_status,
                "model": model,
                "provider": provider,
                "state": state,
                "successes": successes,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        failures = d.pop("failures")

        def _parse_last_failure_at(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        last_failure_at = _parse_last_failure_at(d.pop("last_failure_at"))

        def _parse_last_status(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        last_status = _parse_last_status(d.pop("last_status"))

        model = d.pop("model")

        provider = d.pop("provider")

        state = TargetState(d.pop("state"))

        successes = d.pop("successes")

        target_health = cls(
            failures=failures,
            last_failure_at=last_failure_at,
            last_status=last_status,
            model=model,
            provider=provider,
            state=state,
            successes=successes,
        )

        target_health.additional_properties = d
        return target_health

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
