from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.api_error_detail_fields import ApiErrorDetailFields


T = TypeVar("T", bound="ApiErrorDetail")


@_attrs_define
class ApiErrorDetail:
    """
    Attributes:
        code (str): A stable name for the error, such as `not_found`.
        message (str): Text for a person. It may change.
        fields (ApiErrorDetailFields | Unset): For `validation_failed` only: a message for each field that is not
            valid.
    """

    code: str
    message: str
    fields: ApiErrorDetailFields | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        code = self.code

        message = self.message

        fields: dict[str, Any] | Unset = UNSET
        if not isinstance(self.fields, Unset):
            fields = self.fields.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "code": code,
                "message": message,
            }
        )
        if fields is not UNSET:
            field_dict["fields"] = fields

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.api_error_detail_fields import (
            ApiErrorDetailFields,
        )

        d = dict(src_dict)
        code = d.pop("code")

        message = d.pop("message")

        _fields = d.pop("fields", UNSET)
        fields: ApiErrorDetailFields | Unset
        if isinstance(_fields, Unset):
            fields = UNSET
        else:
            fields = ApiErrorDetailFields.from_dict(_fields)

        api_error_detail = cls(
            code=code,
            message=message,
            fields=fields,
        )

        api_error_detail.additional_properties = d
        return api_error_detail

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
