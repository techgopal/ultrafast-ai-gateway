from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="GrantEntry")


@_attrs_define
class GrantEntry:
    """
    Attributes:
        everyone (bool | Unset):
        teams (list[str] | Unset): Names of teams.
        users (list[str] | Unset): Emails of users.
    """

    everyone: bool | Unset = UNSET
    teams: list[str] | Unset = UNSET
    users: list[str] | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        everyone = self.everyone

        teams: list[str] | Unset = UNSET
        if not isinstance(self.teams, Unset):
            teams = self.teams

        users: list[str] | Unset = UNSET
        if not isinstance(self.users, Unset):
            users = self.users

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if everyone is not UNSET:
            field_dict["everyone"] = everyone
        if teams is not UNSET:
            field_dict["teams"] = teams
        if users is not UNSET:
            field_dict["users"] = users

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        everyone = d.pop("everyone", UNSET)

        teams = cast(list[str], d.pop("teams", UNSET))

        users = cast(list[str], d.pop("users", UNSET))

        grant_entry = cls(
            everyone=everyone,
            teams=teams,
            users=users,
        )

        return grant_entry
