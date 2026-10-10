from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..models.database_kind import DatabaseKind

if TYPE_CHECKING:
    from ..models.login_limits import LoginLimits


T = TypeVar("T", bound="SettingsView")


@_attrs_define
class SettingsView:
    """
    Attributes:
        database (DatabaseKind): Which database the gateway runs on.
        log_retention_days (int): How many days request logs are kept before they are deleted.
        login_limits (LoginLimits): The limits of failed sign-ins, as they are built in.
        session_hours (int): How many hours a session lives, from sign-in. Sessions that exist
            keep the lifetime they were made with.
        trusted_proxies (list[str]): The networks (CIDR) whose forwarding headers are believed, as the
            gateway was started. Read only: it is a flag of `ultrafast serve`.
    """

    database: DatabaseKind
    log_retention_days: int
    login_limits: LoginLimits
    session_hours: int
    trusted_proxies: list[str]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        database = self.database.value

        log_retention_days = self.log_retention_days

        login_limits = self.login_limits.to_dict()

        session_hours = self.session_hours

        trusted_proxies = self.trusted_proxies

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "database": database,
                "log_retention_days": log_retention_days,
                "login_limits": login_limits,
                "session_hours": session_hours,
                "trusted_proxies": trusted_proxies,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.login_limits import LoginLimits

        d = dict(src_dict)
        database = DatabaseKind(d.pop("database"))

        log_retention_days = d.pop("log_retention_days")

        login_limits = LoginLimits.from_dict(d.pop("login_limits"))

        session_hours = d.pop("session_hours")

        trusted_proxies = cast(list[str], d.pop("trusted_proxies"))

        settings_view = cls(
            database=database,
            log_retention_days=log_retention_days,
            login_limits=login_limits,
            session_hours=session_hours,
            trusted_proxies=trusted_proxies,
        )

        settings_view.additional_properties = d
        return settings_view

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
