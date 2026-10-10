from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.update_rule_request_params_type_0 import UpdateRuleRequestParamsType0


T = TypeVar("T", bound="UpdateRuleRequest")


@_attrs_define
class UpdateRuleRequest:
    """
    Attributes:
        channel_ids (list[int] | None | Unset):
        enabled (bool | None | Unset):
        kind (None | str | Unset): Only the kind the rule has; it cannot change.
        name (None | str | Unset):
        params (None | Unset | UpdateRuleRequestParamsType0): Replaces the parameters. What the rule is firing for is
            forgotten.
    """

    channel_ids: list[int] | None | Unset = UNSET
    enabled: bool | None | Unset = UNSET
    kind: None | str | Unset = UNSET
    name: None | str | Unset = UNSET
    params: None | Unset | UpdateRuleRequestParamsType0 = UNSET

    def to_dict(self) -> dict[str, Any]:
        from ..models.update_rule_request_params_type_0 import (
            UpdateRuleRequestParamsType0,
        )

        channel_ids: list[int] | None | Unset
        if isinstance(self.channel_ids, Unset):
            channel_ids = UNSET
        elif isinstance(self.channel_ids, list):
            channel_ids = self.channel_ids

        else:
            channel_ids = self.channel_ids

        enabled: bool | None | Unset
        if isinstance(self.enabled, Unset):
            enabled = UNSET
        else:
            enabled = self.enabled

        kind: None | str | Unset
        if isinstance(self.kind, Unset):
            kind = UNSET
        else:
            kind = self.kind

        name: None | str | Unset
        if isinstance(self.name, Unset):
            name = UNSET
        else:
            name = self.name

        params: dict[str, Any] | None | Unset
        if isinstance(self.params, Unset):
            params = UNSET
        elif isinstance(self.params, UpdateRuleRequestParamsType0):
            params = self.params.to_dict()
        else:
            params = self.params

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if channel_ids is not UNSET:
            field_dict["channel_ids"] = channel_ids
        if enabled is not UNSET:
            field_dict["enabled"] = enabled
        if kind is not UNSET:
            field_dict["kind"] = kind
        if name is not UNSET:
            field_dict["name"] = name
        if params is not UNSET:
            field_dict["params"] = params

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.update_rule_request_params_type_0 import (
            UpdateRuleRequestParamsType0,
        )

        d = dict(src_dict)

        def _parse_channel_ids(data: object) -> list[int] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                channel_ids_type_0 = cast(list[int], data)

                return channel_ids_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[int] | None | Unset, data)

        channel_ids = _parse_channel_ids(d.pop("channel_ids", UNSET))

        def _parse_enabled(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        enabled = _parse_enabled(d.pop("enabled", UNSET))

        def _parse_kind(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        kind = _parse_kind(d.pop("kind", UNSET))

        def _parse_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        name = _parse_name(d.pop("name", UNSET))

        def _parse_params(data: object) -> None | Unset | UpdateRuleRequestParamsType0:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                params_type_0 = UpdateRuleRequestParamsType0.from_dict(data)

                return params_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UpdateRuleRequestParamsType0, data)

        params = _parse_params(d.pop("params", UNSET))

        update_rule_request = cls(
            channel_ids=channel_ids,
            enabled=enabled,
            kind=kind,
            name=name,
            params=params,
        )

        return update_rule_request
