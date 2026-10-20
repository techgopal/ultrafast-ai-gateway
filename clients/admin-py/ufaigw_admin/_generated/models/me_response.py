from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.user_team_view import UserTeamView
    from ..models.user_view import UserView


T = TypeVar("T", bound="MeResponse")


@_attrs_define
class MeResponse:
    """
    Attributes:
        csrf_token (None | str): The CSRF token of the session. `null` for a caller with an access
            token.
        teams (list[UserTeamView]):
        user (UserView): A user as `/api` shows it. It has no field for the password hash.
    """

    csrf_token: None | str
    teams: list[UserTeamView]
    user: UserView
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        csrf_token: None | str
        csrf_token = self.csrf_token

        teams = []
        for teams_item_data in self.teams:
            teams_item = teams_item_data.to_dict()
            teams.append(teams_item)

        user = self.user.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "csrf_token": csrf_token,
                "teams": teams,
                "user": user,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.user_team_view import UserTeamView
        from ..models.user_view import UserView

        d = dict(src_dict)

        def _parse_csrf_token(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        csrf_token = _parse_csrf_token(d.pop("csrf_token"))

        teams = []
        _teams = d.pop("teams")
        for teams_item_data in _teams:
            teams_item = UserTeamView.from_dict(teams_item_data)

            teams.append(teams_item)

        user = UserView.from_dict(d.pop("user"))

        me_response = cls(
            csrf_token=csrf_token,
            teams=teams,
            user=user,
        )

        me_response.additional_properties = d
        return me_response

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
