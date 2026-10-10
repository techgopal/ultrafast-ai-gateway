from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="CreateMcpServerRequest")


@_attrs_define
class CreateMcpServerRequest:
    """
    Attributes:
        name (str): 1 to 63 characters of `a-z`, `0-9` and `-`, starting with a letter or
            a digit. It is the name in the endpoint `/mcp/{name}` and in a key's
            allowlist (`mcp:{name}/{tool}`), and cannot be changed.
        url (str): The streamable-HTTP endpoint of the server.
        auth_header (None | str | Unset): The header that carries the secret, for `auth_kind` `header`.
        auth_kind (None | str | Unset): `none` (the default), `bearer` or `header`.
        description (None | str | Unset):
        enabled (bool | None | Unset): On by default.
        secret (None | str | Unset): Required with `bearer` or `header`.
    """

    name: str
    url: str
    auth_header: None | str | Unset = UNSET
    auth_kind: None | str | Unset = UNSET
    description: None | str | Unset = UNSET
    enabled: bool | None | Unset = UNSET
    secret: None | str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        url = self.url

        auth_header: None | str | Unset
        if isinstance(self.auth_header, Unset):
            auth_header = UNSET
        else:
            auth_header = self.auth_header

        auth_kind: None | str | Unset
        if isinstance(self.auth_kind, Unset):
            auth_kind = UNSET
        else:
            auth_kind = self.auth_kind

        description: None | str | Unset
        if isinstance(self.description, Unset):
            description = UNSET
        else:
            description = self.description

        enabled: bool | None | Unset
        if isinstance(self.enabled, Unset):
            enabled = UNSET
        else:
            enabled = self.enabled

        secret: None | str | Unset
        if isinstance(self.secret, Unset):
            secret = UNSET
        else:
            secret = self.secret

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "name": name,
                "url": url,
            }
        )
        if auth_header is not UNSET:
            field_dict["auth_header"] = auth_header
        if auth_kind is not UNSET:
            field_dict["auth_kind"] = auth_kind
        if description is not UNSET:
            field_dict["description"] = description
        if enabled is not UNSET:
            field_dict["enabled"] = enabled
        if secret is not UNSET:
            field_dict["secret"] = secret

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        name = d.pop("name")

        url = d.pop("url")

        def _parse_auth_header(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        auth_header = _parse_auth_header(d.pop("auth_header", UNSET))

        def _parse_auth_kind(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        auth_kind = _parse_auth_kind(d.pop("auth_kind", UNSET))

        def _parse_description(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        description = _parse_description(d.pop("description", UNSET))

        def _parse_enabled(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        enabled = _parse_enabled(d.pop("enabled", UNSET))

        def _parse_secret(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        secret = _parse_secret(d.pop("secret", UNSET))

        create_mcp_server_request = cls(
            name=name,
            url=url,
            auth_header=auth_header,
            auth_kind=auth_kind,
            description=description,
            enabled=enabled,
            secret=secret,
        )

        return create_mcp_server_request
