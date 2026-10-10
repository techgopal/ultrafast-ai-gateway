from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.guardrail_view import GuardrailView


T = TypeVar("T", bound="CreatedGuardrail")


@_attrs_define
class CreatedGuardrail:
    """A created guardrail and, for an external one, its signing secret.

    Attributes:
        guardrail (GuardrailView): A guardrail as `/api` shows it: the host of an external one's URL, never
            the URL or the secret.
        secret (None | str): The signing secret of an `external` guardrail. It is shown once, in
            this answer, and cannot be read again. `null` for `rules`.
    """

    guardrail: GuardrailView
    secret: None | str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        guardrail = self.guardrail.to_dict()

        secret: None | str
        secret = self.secret

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "guardrail": guardrail,
                "secret": secret,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.guardrail_view import GuardrailView

        d = dict(src_dict)
        guardrail = GuardrailView.from_dict(d.pop("guardrail"))

        def _parse_secret(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        secret = _parse_secret(d.pop("secret"))

        created_guardrail = cls(
            guardrail=guardrail,
            secret=secret,
        )

        created_guardrail.additional_properties = d
        return created_guardrail

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
