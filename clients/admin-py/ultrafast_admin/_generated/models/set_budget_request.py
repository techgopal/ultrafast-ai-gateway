from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="SetBudgetRequest")


@_attrs_define
class SetBudgetRequest:
    """
    Attributes:
        action (str): `block` or `alert`.
        amount_micros (int): The amount in millionths of a dollar: 1 to 1 000 000 000 000 000.
        period (str): `daily`, `weekly` or `monthly`.
        scope (str): `gateway`, `team`, `user` or `key`.
        scope_id (int | None | Unset): The id of the team, user or key. Not sent for the gateway.
    """

    action: str
    amount_micros: int
    period: str
    scope: str
    scope_id: int | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        action = self.action

        amount_micros = self.amount_micros

        period = self.period

        scope = self.scope

        scope_id: int | None | Unset
        if isinstance(self.scope_id, Unset):
            scope_id = UNSET
        else:
            scope_id = self.scope_id

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "action": action,
                "amount_micros": amount_micros,
                "period": period,
                "scope": scope,
            }
        )
        if scope_id is not UNSET:
            field_dict["scope_id"] = scope_id

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        action = d.pop("action")

        amount_micros = d.pop("amount_micros")

        period = d.pop("period")

        scope = d.pop("scope")

        def _parse_scope_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        scope_id = _parse_scope_id(d.pop("scope_id", UNSET))

        set_budget_request = cls(
            action=action,
            amount_micros=amount_micros,
            period=period,
            scope=scope,
            scope_id=scope_id,
        )

        return set_budget_request
