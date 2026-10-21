from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

T = TypeVar("T", bound="BudgetEntry")


@_attrs_define
class BudgetEntry:
    """
    Attributes:
        action (str): `block` or `alert`.
        amount_micros (int):
        name (None | str): The team's name or the user's email; `null` for the gateway.
        period (str): `daily`, `weekly` or `monthly`.
        scope (str): `gateway`, `team` or `user`.
    """

    action: str
    amount_micros: int
    name: None | str
    period: str
    scope: str

    def to_dict(self) -> dict[str, Any]:
        action = self.action

        amount_micros = self.amount_micros

        name: None | str
        name = self.name

        period = self.period

        scope = self.scope

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "action": action,
                "amount_micros": amount_micros,
                "name": name,
                "period": period,
                "scope": scope,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        action = d.pop("action")

        amount_micros = d.pop("amount_micros")

        def _parse_name(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        name = _parse_name(d.pop("name"))

        period = d.pop("period")

        scope = d.pop("scope")

        budget_entry = cls(
            action=action,
            amount_micros=amount_micros,
            name=name,
            period=period,
            scope=scope,
        )

        return budget_entry
