from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="BudgetView")


@_attrs_define
class BudgetView:
    """The budget of one team, user, key or of the gateway for one period.

    Attributes:
        action (str): `block` refuses calls once the amount is spent; `alert` allows them
            and writes one audit entry per period.
        amount_micros (int): The amount, in millionths of a dollar.
        id (int):
        label (str): How a message names it: `gateway`, `team 'Platform'`,
            `user 'lena@example.com'` or `key 'ci'`.
        period (str): `daily`, `weekly` (from Monday) or `monthly`; UTC calendar periods.
        period_start (str): The UTC date the current period began on, `YYYY-MM-DD`.
        scope (str): `gateway`, `team`, `user` or `key`.
        scope_id (int | None): The id of the team, user or key; `null` for the gateway.
        spent_micros (int | None): What the gateway counted as spent in the current period, in
            millionths of a dollar. `null` when the caller may not see it: a
            member sees the spend of their own user and keys and of no one else's,
            nor of the gateway or of a team they do not lead.
    """

    action: str
    amount_micros: int
    id: int
    label: str
    period: str
    period_start: str
    scope: str
    scope_id: int | None
    spent_micros: int | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        action = self.action

        amount_micros = self.amount_micros

        id = self.id

        label = self.label

        period = self.period

        period_start = self.period_start

        scope = self.scope

        scope_id: int | None
        scope_id = self.scope_id

        spent_micros: int | None
        spent_micros = self.spent_micros

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "action": action,
                "amount_micros": amount_micros,
                "id": id,
                "label": label,
                "period": period,
                "period_start": period_start,
                "scope": scope,
                "scope_id": scope_id,
                "spent_micros": spent_micros,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        action = d.pop("action")

        amount_micros = d.pop("amount_micros")

        id = d.pop("id")

        label = d.pop("label")

        period = d.pop("period")

        period_start = d.pop("period_start")

        scope = d.pop("scope")

        def _parse_scope_id(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        scope_id = _parse_scope_id(d.pop("scope_id"))

        def _parse_spent_micros(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        spent_micros = _parse_spent_micros(d.pop("spent_micros"))

        budget_view = cls(
            action=action,
            amount_micros=amount_micros,
            id=id,
            label=label,
            period=period,
            period_start=period_start,
            scope=scope,
            scope_id=scope_id,
            spent_micros=spent_micros,
        )

        budget_view.additional_properties = d
        return budget_view

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
