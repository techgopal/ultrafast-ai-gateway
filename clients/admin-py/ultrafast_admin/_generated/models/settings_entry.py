from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

T = TypeVar("T", bound="SettingsEntry")


@_attrs_define
class SettingsEntry:
    """
    Attributes:
        log_retention_days (int | None): 1 to 3650. Not in the file: not changed.
        session_hours (int | None): 1 to 720. Not in the file: not changed.
    """

    log_retention_days: int | None
    session_hours: int | None

    def to_dict(self) -> dict[str, Any]:
        log_retention_days: int | None
        log_retention_days = self.log_retention_days

        session_hours: int | None
        session_hours = self.session_hours

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "log_retention_days": log_retention_days,
                "session_hours": session_hours,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_log_retention_days(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        log_retention_days = _parse_log_retention_days(d.pop("log_retention_days"))

        def _parse_session_hours(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        session_hours = _parse_session_hours(d.pop("session_hours"))

        settings_entry = cls(
            log_retention_days=log_retention_days,
            session_hours=session_hours,
        )

        return settings_entry
