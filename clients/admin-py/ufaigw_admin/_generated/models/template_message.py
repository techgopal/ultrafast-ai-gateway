from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from typing_extensions import Self

T = TypeVar("T", bound="TemplateMessage")


@_attrs_define
class TemplateMessage:
    """A message of a version, as stored.

    Attributes:
        content (str): Text, with `{{name}}` where a value goes.
        role (str): `system`, `developer`, `user` or `assistant`.
    """

    content: str
    role: str

    def to_dict(self) -> dict[str, Any]:
        content = self.content

        role = self.role

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "content": content,
                "role": role,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        content = d.pop("content")

        role = d.pop("role")

        template_message = cls(
            content=content,
            role=role,
        )

        return template_message
