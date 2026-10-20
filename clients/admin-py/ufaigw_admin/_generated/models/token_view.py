from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..models.token_status import TokenStatus

T = TypeVar("T", bound="TokenView")


@_attrs_define
class TokenView:
    """A token as `/api` shows it. It has no field for the token or its hash.

    Attributes:
        created_at (str):
        display (str):
        expires_at (None | str):
        id (int):
        last_used_at (None | str):
        name (str):
        revoked_at (None | str):
        status (TokenStatus): Whether a token can still be used, as of the moment of the answer.
    """

    created_at: str
    display: str
    expires_at: None | str
    id: int
    last_used_at: None | str
    name: str
    revoked_at: None | str
    status: TokenStatus
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        created_at = self.created_at

        display = self.display

        expires_at: None | str
        expires_at = self.expires_at

        id = self.id

        last_used_at: None | str
        last_used_at = self.last_used_at

        name = self.name

        revoked_at: None | str
        revoked_at = self.revoked_at

        status = self.status.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "created_at": created_at,
                "display": display,
                "expires_at": expires_at,
                "id": id,
                "last_used_at": last_used_at,
                "name": name,
                "revoked_at": revoked_at,
                "status": status,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        created_at = d.pop("created_at")

        display = d.pop("display")

        def _parse_expires_at(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        expires_at = _parse_expires_at(d.pop("expires_at"))

        id = d.pop("id")

        def _parse_last_used_at(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        last_used_at = _parse_last_used_at(d.pop("last_used_at"))

        name = d.pop("name")

        def _parse_revoked_at(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        revoked_at = _parse_revoked_at(d.pop("revoked_at"))

        status = TokenStatus(d.pop("status"))

        token_view = cls(
            created_at=created_at,
            display=display,
            expires_at=expires_at,
            id=id,
            last_used_at=last_used_at,
            name=name,
            revoked_at=revoked_at,
            status=status,
        )

        token_view.additional_properties = d
        return token_view

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
