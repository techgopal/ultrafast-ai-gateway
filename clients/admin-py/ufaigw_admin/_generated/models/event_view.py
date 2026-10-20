from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.event_view_deliveries_item import EventViewDeliveriesItem
    from ..models.event_view_details import EventViewDetails


T = TypeVar("T", bound="EventView")


@_attrs_define
class EventView:
    """One notification, as `/api` shows it. Metadata only.

    Attributes:
        at (str): UTC, `YYYY-MM-DD HH:MM:SS`.
        deliveries (list[EventViewDeliveriesItem]): One entry per channel once every delivery finished:
            `{channel_id, channel_name, ok, status, tries, error}`. Empty before.
        details (EventViewDetails): What the alert is about, by kind.
        id (int):
        kind (str):
        rule_id (int | None): `null` once the rule is deleted.
        rule_name (str):
        state (str): `firing`, `resolved` or `test`.
        subject (str):
        summary (str):
    """

    at: str
    deliveries: list[EventViewDeliveriesItem]
    details: EventViewDetails
    id: int
    kind: str
    rule_id: int | None
    rule_name: str
    state: str
    subject: str
    summary: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        at = self.at

        deliveries = []
        for deliveries_item_data in self.deliveries:
            deliveries_item = deliveries_item_data.to_dict()
            deliveries.append(deliveries_item)

        details = self.details.to_dict()

        id = self.id

        kind = self.kind

        rule_id: int | None
        rule_id = self.rule_id

        rule_name = self.rule_name

        state = self.state

        subject = self.subject

        summary = self.summary

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "at": at,
                "deliveries": deliveries,
                "details": details,
                "id": id,
                "kind": kind,
                "rule_id": rule_id,
                "rule_name": rule_name,
                "state": state,
                "subject": subject,
                "summary": summary,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.event_view_deliveries_item import (
            EventViewDeliveriesItem,
        )
        from ..models.event_view_details import EventViewDetails

        d = dict(src_dict)
        at = d.pop("at")

        deliveries = []
        _deliveries = d.pop("deliveries")
        for deliveries_item_data in _deliveries:
            deliveries_item = EventViewDeliveriesItem.from_dict(deliveries_item_data)

            deliveries.append(deliveries_item)

        details = EventViewDetails.from_dict(d.pop("details"))

        id = d.pop("id")

        kind = d.pop("kind")

        def _parse_rule_id(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        rule_id = _parse_rule_id(d.pop("rule_id"))

        rule_name = d.pop("rule_name")

        state = d.pop("state")

        subject = d.pop("subject")

        summary = d.pop("summary")

        event_view = cls(
            at=at,
            deliveries=deliveries,
            details=details,
            id=id,
            kind=kind,
            rule_id=rule_id,
            rule_name=rule_name,
            state=state,
            subject=subject,
            summary=summary,
        )

        event_view.additional_properties = d
        return event_view

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
