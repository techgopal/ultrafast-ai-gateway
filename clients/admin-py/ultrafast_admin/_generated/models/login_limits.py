from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="LoginLimits")


@_attrs_define
class LoginLimits:
    """The limits of failed sign-ins, as they are built in.

    Attributes:
        max_per_address (int): Failures one client address may have inside the window.
        max_per_email (int): Failures one email may have inside the window.
        window_minutes (int): Failures older than this no longer count.
    """

    max_per_address: int
    max_per_email: int
    window_minutes: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        max_per_address = self.max_per_address

        max_per_email = self.max_per_email

        window_minutes = self.window_minutes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "max_per_address": max_per_address,
                "max_per_email": max_per_email,
                "window_minutes": window_minutes,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        max_per_address = d.pop("max_per_address")

        max_per_email = d.pop("max_per_email")

        window_minutes = d.pop("window_minutes")

        login_limits = cls(
            max_per_address=max_per_address,
            max_per_email=max_per_email,
            window_minutes=window_minutes,
        )

        login_limits.additional_properties = d
        return login_limits

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
