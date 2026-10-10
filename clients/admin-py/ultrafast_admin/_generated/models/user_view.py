from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..models.auth_provider_view import AuthProviderView
from ..models.role import Role
from ..models.user_status import UserStatus

if TYPE_CHECKING:
    from ..models.user_team_view import UserTeamView


T = TypeVar("T", bound="UserView")


@_attrs_define
class UserView:
    """A user as `/api` shows it. It has no field for the password hash.

    Attributes:
        auth_provider (AuthProviderView): How a user signs in, as `/api` shows it.
        created_at (str):
        email (str):
        has_password (bool): Whether the user has a password. A user made by single sign-on has
            none and can sign in only while single sign-on works. Read only.
        id (int):
        last_active_at (None | str):
        name (str):
        role (Role): A user's role in the organization.
        status (UserStatus): The state of a user account.
        teams (list[UserTeamView]): The user's teams, ordered by name.
    """

    auth_provider: AuthProviderView
    created_at: str
    email: str
    has_password: bool
    id: int
    last_active_at: None | str
    name: str
    role: Role
    status: UserStatus
    teams: list[UserTeamView]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        auth_provider = self.auth_provider.value

        created_at = self.created_at

        email = self.email

        has_password = self.has_password

        id = self.id

        last_active_at: None | str
        last_active_at = self.last_active_at

        name = self.name

        role = self.role.value

        status = self.status.value

        teams = []
        for teams_item_data in self.teams:
            teams_item = teams_item_data.to_dict()
            teams.append(teams_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "auth_provider": auth_provider,
                "created_at": created_at,
                "email": email,
                "has_password": has_password,
                "id": id,
                "last_active_at": last_active_at,
                "name": name,
                "role": role,
                "status": status,
                "teams": teams,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.user_team_view import UserTeamView

        d = dict(src_dict)
        auth_provider = AuthProviderView(d.pop("auth_provider"))

        created_at = d.pop("created_at")

        email = d.pop("email")

        has_password = d.pop("has_password")

        id = d.pop("id")

        def _parse_last_active_at(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        last_active_at = _parse_last_active_at(d.pop("last_active_at"))

        name = d.pop("name")

        role = Role(d.pop("role"))

        status = UserStatus(d.pop("status"))

        teams = []
        _teams = d.pop("teams")
        for teams_item_data in _teams:
            teams_item = UserTeamView.from_dict(teams_item_data)

            teams.append(teams_item)

        user_view = cls(
            auth_provider=auth_provider,
            created_at=created_at,
            email=email,
            has_password=has_password,
            id=id,
            last_active_at=last_active_at,
            name=name,
            role=role,
            status=status,
            teams=teams,
        )

        user_view.additional_properties = d
        return user_view

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
