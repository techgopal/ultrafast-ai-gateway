from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

T = TypeVar("T", bound="GrantsView")


@_attrs_define
class GrantsView:
    """Who may call a model. For everyone who is not an admin it is always
    empty.

        Attributes:
            everyone (bool): Every user may call the model. It cannot be combined with teams or
                users.
            team_ids (list[int]):
            user_ids (list[int]):
    """

    everyone: bool
    team_ids: list[int]
    user_ids: list[int]

    def to_dict(self) -> dict[str, Any]:
        everyone = self.everyone

        team_ids = self.team_ids

        user_ids = self.user_ids

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "everyone": everyone,
                "team_ids": team_ids,
                "user_ids": user_ids,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        everyone = d.pop("everyone")

        team_ids = cast(list[int], d.pop("team_ids"))

        user_ids = cast(list[int], d.pop("user_ids"))

        grants_view = cls(
            everyone=everyone,
            team_ids=team_ids,
            user_ids=user_ids,
        )

        return grants_view
