from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="AuditRow")


@_attrs_define
class AuditRow:
    """
    Attributes:
        action (str):
        actor_email (str):
        at (str):
        id (int):
        summary (str):
        target_id (int | None):
        target_type (str):
    """

    action: str
    actor_email: str
    at: str
    id: int
    summary: str
    target_id: int | None
    target_type: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        action = self.action

        actor_email = self.actor_email

        at = self.at

        id = self.id

        summary = self.summary

        target_id: int | None
        target_id = self.target_id

        target_type = self.target_type

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "action": action,
                "actor_email": actor_email,
                "at": at,
                "id": id,
                "summary": summary,
                "target_id": target_id,
                "target_type": target_type,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        action = d.pop("action")

        actor_email = d.pop("actor_email")

        at = d.pop("at")

        id = d.pop("id")

        summary = d.pop("summary")

        def _parse_target_id(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        target_id = _parse_target_id(d.pop("target_id"))

        target_type = d.pop("target_type")

        audit_row = cls(
            action=action,
            actor_email=actor_email,
            at=at,
            id=id,
            summary=summary,
            target_id=target_id,
            target_type=target_type,
        )

        audit_row.additional_properties = d
        return audit_row

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
