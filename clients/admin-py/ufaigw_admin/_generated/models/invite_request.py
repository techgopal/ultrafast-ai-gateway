from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from typing_extensions import Self

T = TypeVar("T", bound="InviteRequest")


@_attrs_define
class InviteRequest:
    """
    Attributes:
        email (str):
        name (str):
        role (str):
    """

    email: str
    name: str
    role: str

    def to_dict(self) -> dict[str, Any]:
        email = self.email

        name = self.name

        role = self.role

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "email": email,
                "name": name,
                "role": role,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        email = d.pop("email")

        name = d.pop("name")

        role = d.pop("role")

        invite_request = cls(
            email=email,
            name=name,
            role=role,
        )

        return invite_request
