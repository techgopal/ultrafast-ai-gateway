from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.member_detail import MemberDetail
    from ..models.team_summary import TeamSummary


T = TypeVar("T", bound="TeamDetail")


@_attrs_define
class TeamDetail:
    """
    Attributes:
        members (list[MemberDetail]):
        team (TeamSummary): A team with the number of its members.
    """

    members: list[MemberDetail]
    team: TeamSummary
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        members = []
        for members_item_data in self.members:
            members_item = members_item_data.to_dict()
            members.append(members_item)

        team = self.team.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "members": members,
                "team": team,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.member_detail import MemberDetail
        from ..models.team_summary import TeamSummary

        d = dict(src_dict)
        members = []
        _members = d.pop("members")
        for members_item_data in _members:
            members_item = MemberDetail.from_dict(members_item_data)

            members.append(members_item)

        team = TeamSummary.from_dict(d.pop("team"))

        team_detail = cls(
            members=members,
            team=team,
        )

        team_detail.additional_properties = d
        return team_detail

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
