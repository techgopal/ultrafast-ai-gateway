from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

T = TypeVar("T", bound="AttachRequest")


@_attrs_define
class AttachRequest:
    """The body that sets the guardrails of a team or a user.

    Attributes:
        guardrail_ids (list[int]): The guardrails to apply, in this order, to every key of the team, or
            every key the user owns. `[]` takes them all off. At most 20, each
            one an existing guardrail.
    """

    guardrail_ids: list[int]

    def to_dict(self) -> dict[str, Any]:
        guardrail_ids = self.guardrail_ids

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "guardrail_ids": guardrail_ids,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        guardrail_ids = cast(list[int], d.pop("guardrail_ids"))

        attach_request = cls(
            guardrail_ids=guardrail_ids,
        )

        return attach_request
