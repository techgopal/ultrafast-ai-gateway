from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

T = TypeVar("T", bound="OauthEntry")


@_attrs_define
class OauthEntry:
    """How the gateway signs in to an MCP server with OAuth (see `mcp::oauth`).

    Attributes:
        client_id (str):
        grant (str): `client_credentials` or `token_exchange`.
        scopes (None | str):
        token_url (str):
    """

    client_id: str
    grant: str
    scopes: None | str
    token_url: str

    def to_dict(self) -> dict[str, Any]:
        client_id = self.client_id

        grant = self.grant

        scopes: None | str
        scopes = self.scopes

        token_url = self.token_url

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "client_id": client_id,
                "grant": grant,
                "scopes": scopes,
                "token_url": token_url,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        client_id = d.pop("client_id")

        grant = d.pop("grant")

        def _parse_scopes(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        scopes = _parse_scopes(d.pop("scopes"))

        token_url = d.pop("token_url")

        oauth_entry = cls(
            client_id=client_id,
            grant=grant,
            scopes=scopes,
            token_url=token_url,
        )

        return oauth_entry
