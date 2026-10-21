from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="OidcTestRequest")


@_attrs_define
class OidcTestRequest:
    """
    Attributes:
        issuer (str | Unset): The issuer to test. Left out or empty: the saved one.
    """

    issuer: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        issuer = self.issuer

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if issuer is not UNSET:
            field_dict["issuer"] = issuer

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        issuer = d.pop("issuer", UNSET)

        oidc_test_request = cls(
            issuer=issuer,
        )

        return oidc_test_request
