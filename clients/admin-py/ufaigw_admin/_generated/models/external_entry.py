from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from typing_extensions import Self

from ..models.directions import Directions
from ..types import UNSET, Unset

T = TypeVar("T", bound="ExternalEntry")


@_attrs_define
class ExternalEntry:
    """How an external guardrail behaves. Its URL and signing secret are never
    in a file.

        Attributes:
            directions (Directions | Unset): The directions a rule applies to.
            fail_mode (str | Unset): `open` or `closed`. Not in the file: `open`.
            timeout_ms (int | Unset): 1 000 to 10 000. Not in the file: 3 000.
    """

    directions: Directions | Unset = UNSET
    fail_mode: str | Unset = UNSET
    timeout_ms: int | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        directions: str | Unset = UNSET
        if not isinstance(self.directions, Unset):
            directions = self.directions.value

        fail_mode = self.fail_mode

        timeout_ms = self.timeout_ms

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if directions is not UNSET:
            field_dict["directions"] = directions
        if fail_mode is not UNSET:
            field_dict["fail_mode"] = fail_mode
        if timeout_ms is not UNSET:
            field_dict["timeout_ms"] = timeout_ms

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        _directions = d.pop("directions", UNSET)
        directions: Directions | Unset
        if isinstance(_directions, Unset):
            directions = UNSET
        else:
            directions = Directions(_directions)

        fail_mode = d.pop("fail_mode", UNSET)

        timeout_ms = d.pop("timeout_ms", UNSET)

        external_entry = cls(
            directions=directions,
            fail_mode=fail_mode,
            timeout_ms=timeout_ms,
        )

        return external_entry
