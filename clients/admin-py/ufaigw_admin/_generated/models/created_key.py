from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.key_view import KeyView


T = TypeVar("T", bound="CreatedKey")


@_attrs_define
class CreatedKey:
    """
    Attributes:
        key (KeyView): A key as `/api` shows it. It has no field for the key or its hash.
        secret (str): The virtual key itself. It is shown once, in this answer, and
            cannot be read again.
    """

    key: KeyView
    secret: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        key = self.key.to_dict()

        secret = self.secret

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "key": key,
                "secret": secret,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.key_view import KeyView

        d = dict(src_dict)
        key = KeyView.from_dict(d.pop("key"))

        secret = d.pop("secret")

        created_key = cls(
            key=key,
            secret=secret,
        )

        created_key.additional_properties = d
        return created_key

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
