from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.token_view import TokenView


T = TypeVar("T", bound="CreatedToken")


@_attrs_define
class CreatedToken:
    """
    Attributes:
        secret (str): The access token itself. It is shown once, in this answer, and
            cannot be read again.
        token (TokenView): A token as `/api` shows it. It has no field for the token or its hash.
    """

    secret: str
    token: TokenView
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        secret = self.secret

        token = self.token.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "secret": secret,
                "token": token,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.token_view import TokenView

        d = dict(src_dict)
        secret = d.pop("secret")

        token = TokenView.from_dict(d.pop("token"))

        created_token = cls(
            secret=secret,
            token=token,
        )

        created_token.additional_properties = d
        return created_token

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
