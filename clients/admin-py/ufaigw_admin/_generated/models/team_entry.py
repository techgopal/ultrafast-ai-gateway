from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="TeamEntry")


@_attrs_define
class TeamEntry:
    """
    Attributes:
        name (str):
        guardrails (list[str] | None | Unset): Names of guardrails, in the order they apply to every key of the
            team. Left out of the file for a team that has none; a file that
            leaves it out does not change what is attached (`[]` takes them all
            off).
    """

    name: str
    guardrails: list[str] | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        guardrails: list[str] | None | Unset
        if isinstance(self.guardrails, Unset):
            guardrails = UNSET
        elif isinstance(self.guardrails, list):
            guardrails = self.guardrails

        else:
            guardrails = self.guardrails

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "name": name,
            }
        )
        if guardrails is not UNSET:
            field_dict["guardrails"] = guardrails

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        name = d.pop("name")

        def _parse_guardrails(data: object) -> list[str] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                guardrails_type_0 = cast(list[str], data)

                return guardrails_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None | Unset, data)

        guardrails = _parse_guardrails(d.pop("guardrails", UNSET))

        team_entry = cls(
            name=name,
            guardrails=guardrails,
        )

        return team_entry
