from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..models.logged_action import LoggedAction
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.flag_log import FlagLog
    from ..models.guardrail_ref import GuardrailRef
    from ..models.side_log_redactions import SideLogRedactions


T = TypeVar("T", bound="SideLog")


@_attrs_define
class SideLog:
    """The checks of one direction (the input of a call, or its output).

    Attributes:
        action (LoggedAction): What the checks of a call did, worst first: a block, else a redaction,
            else a flag. The order is the order of severity.
        checked_with (list[GuardrailRef]): The guardrails this direction was checked with.
        blocked_by (GuardrailRef | None | Unset):
        flags (list[FlagLog] | Unset): Flag rules that matched.
        redactions (SideLogRedactions | Unset): Replacements made, by PII type or rule id.
    """

    action: LoggedAction
    checked_with: list[GuardrailRef]
    blocked_by: GuardrailRef | None | Unset = UNSET
    flags: list[FlagLog] | Unset = UNSET
    redactions: SideLogRedactions | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.guardrail_ref import GuardrailRef

        action = self.action.value

        checked_with = []
        for checked_with_item_data in self.checked_with:
            checked_with_item = checked_with_item_data.to_dict()
            checked_with.append(checked_with_item)

        blocked_by: dict[str, Any] | None | Unset
        if isinstance(self.blocked_by, Unset):
            blocked_by = UNSET
        elif isinstance(self.blocked_by, GuardrailRef):
            blocked_by = self.blocked_by.to_dict()
        else:
            blocked_by = self.blocked_by

        flags: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.flags, Unset):
            flags = []
            for flags_item_data in self.flags:
                flags_item = flags_item_data.to_dict()
                flags.append(flags_item)

        redactions: dict[str, Any] | Unset = UNSET
        if not isinstance(self.redactions, Unset):
            redactions = self.redactions.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "action": action,
                "checked_with": checked_with,
            }
        )
        if blocked_by is not UNSET:
            field_dict["blocked_by"] = blocked_by
        if flags is not UNSET:
            field_dict["flags"] = flags
        if redactions is not UNSET:
            field_dict["redactions"] = redactions

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.flag_log import FlagLog
        from ..models.guardrail_ref import GuardrailRef
        from ..models.side_log_redactions import SideLogRedactions

        d = dict(src_dict)
        action = LoggedAction(d.pop("action"))

        checked_with = []
        _checked_with = d.pop("checked_with")
        for checked_with_item_data in _checked_with:
            checked_with_item = GuardrailRef.from_dict(checked_with_item_data)

            checked_with.append(checked_with_item)

        def _parse_blocked_by(data: object) -> GuardrailRef | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                blocked_by_type_0 = GuardrailRef.from_dict(data)

                return blocked_by_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(GuardrailRef | None | Unset, data)

        blocked_by = _parse_blocked_by(d.pop("blocked_by", UNSET))

        _flags = d.pop("flags", UNSET)
        flags: list[FlagLog] | Unset = UNSET
        if _flags is not UNSET:
            flags = []
            for flags_item_data in _flags:
                flags_item = FlagLog.from_dict(flags_item_data)

                flags.append(flags_item)

        _redactions = d.pop("redactions", UNSET)
        redactions: SideLogRedactions | Unset
        if isinstance(_redactions, Unset):
            redactions = UNSET
        else:
            redactions = SideLogRedactions.from_dict(_redactions)

        side_log = cls(
            action=action,
            checked_with=checked_with,
            blocked_by=blocked_by,
            flags=flags,
            redactions=redactions,
        )

        side_log.additional_properties = d
        return side_log

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
