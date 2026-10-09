from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from typing_extensions import Self

T = TypeVar("T", bound="CreateModelRequest")


@_attrs_define
class CreateModelRequest:
    """
    Attributes:
        name (str): The provider's id for the model, 1 to 200 characters, no whitespace.
        provider_id (int):
    """

    name: str
    provider_id: int

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        provider_id = self.provider_id

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "name": name,
                "provider_id": provider_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        name = d.pop("name")

        provider_id = d.pop("provider_id")

        create_model_request = cls(
            name=name,
            provider_id=provider_id,
        )

        return create_model_request
