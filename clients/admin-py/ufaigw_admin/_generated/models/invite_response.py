from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.user_view import UserView


T = TypeVar("T", bound="InviteResponse")


@_attrs_define
class InviteResponse:
    """
    Attributes:
        invite_link (str): The link that lets the user set a password. It is shown once, in
            this answer, and cannot be read again.
        user (UserView): A user as `/api` shows it. It has no field for the password hash.
    """

    invite_link: str
    user: UserView
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        invite_link = self.invite_link

        user = self.user.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "invite_link": invite_link,
                "user": user,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.user_view import UserView

        d = dict(src_dict)
        invite_link = d.pop("invite_link")

        user = UserView.from_dict(d.pop("user"))

        invite_response = cls(
            invite_link=invite_link,
            user=user,
        )

        invite_response.additional_properties = d
        return invite_response

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
