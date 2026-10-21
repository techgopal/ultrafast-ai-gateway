from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.grants_view import GrantsView


T = TypeVar("T", bound="ModelView")


@_attrs_define
class ModelView:
    """
    Attributes:
        created_at (str):
        enabled (bool):
        grants (GrantsView): Who may call a model. For everyone who is not an admin it is always
            empty.
        id (int):
        input_price_micros (int | None): What a million input tokens cost, in millionths of a dollar. `null`
            is unknown: calls of the model are logged with cost 0, unpriced.
        name (str): The provider's own id for the model.
        output_price_micros (int | None): What a million output tokens cost, in millionths of a dollar.
        provider_id (int):
        provider_name (str):
    """

    created_at: str
    enabled: bool
    grants: GrantsView
    id: int
    input_price_micros: int | None
    name: str
    output_price_micros: int | None
    provider_id: int
    provider_name: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        created_at = self.created_at

        enabled = self.enabled

        grants = self.grants.to_dict()

        id = self.id

        input_price_micros: int | None
        input_price_micros = self.input_price_micros

        name = self.name

        output_price_micros: int | None
        output_price_micros = self.output_price_micros

        provider_id = self.provider_id

        provider_name = self.provider_name

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "created_at": created_at,
                "enabled": enabled,
                "grants": grants,
                "id": id,
                "input_price_micros": input_price_micros,
                "name": name,
                "output_price_micros": output_price_micros,
                "provider_id": provider_id,
                "provider_name": provider_name,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.grants_view import GrantsView

        d = dict(src_dict)
        created_at = d.pop("created_at")

        enabled = d.pop("enabled")

        grants = GrantsView.from_dict(d.pop("grants"))

        id = d.pop("id")

        def _parse_input_price_micros(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        input_price_micros = _parse_input_price_micros(d.pop("input_price_micros"))

        name = d.pop("name")

        def _parse_output_price_micros(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        output_price_micros = _parse_output_price_micros(d.pop("output_price_micros"))

        provider_id = d.pop("provider_id")

        provider_name = d.pop("provider_name")

        model_view = cls(
            created_at=created_at,
            enabled=enabled,
            grants=grants,
            id=id,
            input_price_micros=input_price_micros,
            name=name,
            output_price_micros=output_price_micros,
            provider_id=provider_id,
            provider_name=provider_name,
        )

        model_view.additional_properties = d
        return model_view

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
