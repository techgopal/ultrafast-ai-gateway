from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from typing_extensions import Self

T = TypeVar("T", bound="AcceptInviteRequest")


@_attrs_define
class AcceptInviteRequest:
    """
    Attributes:
        password (str):
        token (str): The token of the invite link.
    """

    password: str
    token: str

    def to_dict(self) -> dict[str, Any]:
        password = self.password

        token = self.token

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "password": password,
                "token": token,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        password = d.pop("password")

        token = d.pop("token")

        accept_invite_request = cls(
            password=password,
            token=token,
        )

        return accept_invite_request
