from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.render_request_variables import RenderRequestVariables


T = TypeVar("T", bound="RenderRequest")


@_attrs_define
class RenderRequest:
    """
    Attributes:
        variables (RenderRequestVariables | Unset): A value for every variable of the version, and for no other: a text
            of at most 32 KiB each. Put in as it is, once.
        version (int | None | Unset): Left out: the latest version.
    """

    variables: RenderRequestVariables | Unset = UNSET
    version: int | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        variables: dict[str, Any] | Unset = UNSET
        if not isinstance(self.variables, Unset):
            variables = self.variables.to_dict()

        version: int | None | Unset
        if isinstance(self.version, Unset):
            version = UNSET
        else:
            version = self.version

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if variables is not UNSET:
            field_dict["variables"] = variables
        if version is not UNSET:
            field_dict["version"] = version

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.render_request_variables import (
            RenderRequestVariables,
        )

        d = dict(src_dict)
        _variables = d.pop("variables", UNSET)
        variables: RenderRequestVariables | Unset
        if isinstance(_variables, Unset):
            variables = UNSET
        else:
            variables = RenderRequestVariables.from_dict(_variables)

        def _parse_version(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        version = _parse_version(d.pop("version", UNSET))

        render_request = cls(
            variables=variables,
            version=version,
        )

        return render_request
