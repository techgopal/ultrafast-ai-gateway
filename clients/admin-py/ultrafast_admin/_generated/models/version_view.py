from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.params import Params
    from ..models.template_message import TemplateMessage


T = TypeVar("T", bound="VersionView")


@_attrs_define
class VersionView:
    """One version of a template.

    Attributes:
        created_at (str):
        created_by (int | None): `null` when the user is gone.
        messages (list[TemplateMessage]):
        model (None | str):
        params (Params): The settings a version carries for the call. All optional; what the call
            itself sets wins.
        variables (list[str]): The names the messages use, sorted.
        version (int): From 1.
    """

    created_at: str
    created_by: int | None
    messages: list[TemplateMessage]
    model: None | str
    params: Params
    variables: list[str]
    version: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        created_at = self.created_at

        created_by: int | None
        created_by = self.created_by

        messages = []
        for messages_item_data in self.messages:
            messages_item = messages_item_data.to_dict()
            messages.append(messages_item)

        model: None | str
        model = self.model

        params = self.params.to_dict()

        variables = self.variables

        version = self.version

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "created_at": created_at,
                "created_by": created_by,
                "messages": messages,
                "model": model,
                "params": params,
                "variables": variables,
                "version": version,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.params import Params
        from ..models.template_message import TemplateMessage

        d = dict(src_dict)
        created_at = d.pop("created_at")

        def _parse_created_by(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        created_by = _parse_created_by(d.pop("created_by"))

        messages = []
        _messages = d.pop("messages")
        for messages_item_data in _messages:
            messages_item = TemplateMessage.from_dict(messages_item_data)

            messages.append(messages_item)

        def _parse_model(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        model = _parse_model(d.pop("model"))

        params = Params.from_dict(d.pop("params"))

        variables = cast(list[str], d.pop("variables"))

        version = d.pop("version")

        version_view = cls(
            created_at=created_at,
            created_by=created_by,
            messages=messages,
            model=model,
            params=params,
            variables=variables,
            version=version,
        )

        version_view.additional_properties = d
        return version_view

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
