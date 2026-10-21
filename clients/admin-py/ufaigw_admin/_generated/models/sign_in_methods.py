from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.sign_in_method_oidc import SignInMethodOidc


T = TypeVar("T", bound="SignInMethods")


@_attrs_define
class SignInMethods:
    """
    Attributes:
        oidc (None | SignInMethodOidc):
        password (bool): Always true: passwords are never turned off.
    """

    oidc: None | SignInMethodOidc
    password: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.sign_in_method_oidc import SignInMethodOidc

        oidc: dict[str, Any] | None
        if isinstance(self.oidc, SignInMethodOidc):
            oidc = self.oidc.to_dict()
        else:
            oidc = self.oidc

        password = self.password

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "oidc": oidc,
                "password": password,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.sign_in_method_oidc import SignInMethodOidc

        d = dict(src_dict)

        def _parse_oidc(data: object) -> None | SignInMethodOidc:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                oidc_type_0 = SignInMethodOidc.from_dict(data)

                return oidc_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | SignInMethodOidc, data)

        oidc = _parse_oidc(d.pop("oidc"))

        password = d.pop("password")

        sign_in_methods = cls(
            oidc=oidc,
            password=password,
        )

        sign_in_methods.additional_properties = d
        return sign_in_methods

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
