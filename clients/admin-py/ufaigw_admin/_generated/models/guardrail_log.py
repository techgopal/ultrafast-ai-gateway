from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..models.logged_action import LoggedAction
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.side_log import SideLog


T = TypeVar("T", bound="GuardrailLog")


@_attrs_define
class GuardrailLog:
    """The guardrail record of a call: `None` for a direction that found nothing.

    Attributes:
        action (LoggedAction): What the checks of a call did, worst first: a block, else a redaction,
            else a flag. The order is the order of severity.
        input_ (None | SideLog | Unset):
        output (None | SideLog | Unset):
    """

    action: LoggedAction
    input_: None | SideLog | Unset = UNSET
    output: None | SideLog | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.side_log import SideLog

        action = self.action.value

        input_: dict[str, Any] | None | Unset
        if isinstance(self.input_, Unset):
            input_ = UNSET
        elif isinstance(self.input_, SideLog):
            input_ = self.input_.to_dict()
        else:
            input_ = self.input_

        output: dict[str, Any] | None | Unset
        if isinstance(self.output, Unset):
            output = UNSET
        elif isinstance(self.output, SideLog):
            output = self.output.to_dict()
        else:
            output = self.output

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "action": action,
            }
        )
        if input_ is not UNSET:
            field_dict["input"] = input_
        if output is not UNSET:
            field_dict["output"] = output

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.side_log import SideLog

        d = dict(src_dict)
        action = LoggedAction(d.pop("action"))

        def _parse_input_(data: object) -> None | SideLog | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                input_type_0 = SideLog.from_dict(data)

                return input_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | SideLog | Unset, data)

        input_ = _parse_input_(d.pop("input", UNSET))

        def _parse_output(data: object) -> None | SideLog | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                output_type_0 = SideLog.from_dict(data)

                return output_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | SideLog | Unset, data)

        output = _parse_output(d.pop("output", UNSET))

        guardrail_log = cls(
            action=action,
            input_=input_,
            output=output,
        )

        guardrail_log.additional_properties = d
        return guardrail_log

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
