from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.env_var import EnvVar


T = TypeVar("T", bound="Entry")


@_attrs_define
class Entry:
    """
    Attributes:
        args (list[str]):
        auth (str): `none`, `bearer`, `header` or `oauth`.
        auth_header (None | str):
        command (None | str):
        description (str):
        env (list[EnvVar]):
        homepage (str):
        id (str): `^[a-z0-9][a-z0-9-]{0,62}$`; also the default name of the server.
        name (str):
        tags (list[str]):
        transport (str): `http` or `stdio`.
        url (None | str):
    """

    args: list[str]
    auth: str
    auth_header: None | str
    command: None | str
    description: str
    env: list[EnvVar]
    homepage: str
    id: str
    name: str
    tags: list[str]
    transport: str
    url: None | str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        args = self.args

        auth = self.auth

        auth_header: None | str
        auth_header = self.auth_header

        command: None | str
        command = self.command

        description = self.description

        env = []
        for env_item_data in self.env:
            env_item = env_item_data.to_dict()
            env.append(env_item)

        homepage = self.homepage

        id = self.id

        name = self.name

        tags = self.tags

        transport = self.transport

        url: None | str
        url = self.url

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "args": args,
                "auth": auth,
                "auth_header": auth_header,
                "command": command,
                "description": description,
                "env": env,
                "homepage": homepage,
                "id": id,
                "name": name,
                "tags": tags,
                "transport": transport,
                "url": url,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.env_var import EnvVar

        d = dict(src_dict)
        args = cast(list[str], d.pop("args"))

        auth = d.pop("auth")

        def _parse_auth_header(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        auth_header = _parse_auth_header(d.pop("auth_header"))

        def _parse_command(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        command = _parse_command(d.pop("command"))

        description = d.pop("description")

        env = []
        _env = d.pop("env")
        for env_item_data in _env:
            env_item = EnvVar.from_dict(env_item_data)

            env.append(env_item)

        homepage = d.pop("homepage")

        id = d.pop("id")

        name = d.pop("name")

        tags = cast(list[str], d.pop("tags"))

        transport = d.pop("transport")

        def _parse_url(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        url = _parse_url(d.pop("url"))

        entry = cls(
            args=args,
            auth=auth,
            auth_header=auth_header,
            command=command,
            description=description,
            env=env,
            homepage=homepage,
            id=id,
            name=name,
            tags=tags,
            transport=transport,
            url=url,
        )

        entry.additional_properties = d
        return entry

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
