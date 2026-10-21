from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="EnvVar")


@_attrs_define
class EnvVar:
    """
    Attributes:
        help_ (str):
        name (str): The name of the environment variable (stdio) or the one secret that
            authenticates to the server (http).
        secret (bool):
    """

    help_: str
    name: str
    secret: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        help_ = self.help_

        name = self.name

        secret = self.secret

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "help": help_,
                "name": name,
                "secret": secret,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        help_ = d.pop("help")

        name = d.pop("name")

        secret = d.pop("secret")

        env_var = cls(
            help_=help_,
            name=name,
            secret=secret,
        )

        env_var.additional_properties = d
        return env_var

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
