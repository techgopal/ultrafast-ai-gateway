from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="SetupRequest")


@_attrs_define
class SetupRequest:
    """
    Attributes:
        email (str):
        name (str):
        password (str):
        setup_code (None | str | Unset): The one-time code the gateway printed to its log when it started
            without users. Required; left out or wrong, the answer is 403.
    """

    email: str
    name: str
    password: str
    setup_code: None | str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        email = self.email

        name = self.name

        password = self.password

        setup_code: None | str | Unset
        if isinstance(self.setup_code, Unset):
            setup_code = UNSET
        else:
            setup_code = self.setup_code

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "email": email,
                "name": name,
                "password": password,
            }
        )
        if setup_code is not UNSET:
            field_dict["setup_code"] = setup_code

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        email = d.pop("email")

        name = d.pop("name")

        password = d.pop("password")

        def _parse_setup_code(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        setup_code = _parse_setup_code(d.pop("setup_code", UNSET))

        setup_request = cls(
            email=email,
            name=name,
            password=password,
            setup_code=setup_code,
        )

        return setup_request
