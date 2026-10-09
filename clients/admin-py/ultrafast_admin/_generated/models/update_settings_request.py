from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="UpdateSettingsRequest")


@_attrs_define
class UpdateSettingsRequest:
    """
    Attributes:
        log_retention_days (int | None | Unset): 1 to 3650.
        session_hours (int | None | Unset): 1 to 720. Applies to sessions made from now on.
    """

    log_retention_days: int | None | Unset = UNSET
    session_hours: int | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        log_retention_days: int | None | Unset
        if isinstance(self.log_retention_days, Unset):
            log_retention_days = UNSET
        else:
            log_retention_days = self.log_retention_days

        session_hours: int | None | Unset
        if isinstance(self.session_hours, Unset):
            session_hours = UNSET
        else:
            session_hours = self.session_hours

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if log_retention_days is not UNSET:
            field_dict["log_retention_days"] = log_retention_days
        if session_hours is not UNSET:
            field_dict["session_hours"] = session_hours

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_log_retention_days(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        log_retention_days = _parse_log_retention_days(
            d.pop("log_retention_days", UNSET)
        )

        def _parse_session_hours(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        session_hours = _parse_session_hours(d.pop("session_hours", UNSET))

        update_settings_request = cls(
            log_retention_days=log_retention_days,
            session_hours=session_hours,
        )

        return update_settings_request
