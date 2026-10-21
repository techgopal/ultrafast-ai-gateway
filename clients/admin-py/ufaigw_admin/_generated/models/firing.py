from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="Firing")


@_attrs_define
class Firing:
    """A subject a rule is firing for.

    Attributes:
        since (str): UTC, `YYYY-MM-DD HH:MM:SS`.
        subject (str): `budget:<id>:<period start>`, `route:<name>`, `provider:<name>`,
            `key:<id>`, `gateway` or `target:<provider>/<model>`.
    """

    since: str
    subject: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        since = self.since

        subject = self.subject

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "since": since,
                "subject": subject,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        since = d.pop("since")

        subject = d.pop("subject")

        firing = cls(
            since=since,
            subject=subject,
        )

        firing.additional_properties = d
        return firing

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
