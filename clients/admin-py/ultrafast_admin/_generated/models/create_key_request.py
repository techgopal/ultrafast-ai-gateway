from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.create_key_request_tags_type_0 import CreateKeyRequestTagsType0


T = TypeVar("T", bound="CreateKeyRequest")


@_attrs_define
class CreateKeyRequest:
    """
    Attributes:
        name (str):
        allowed (list[str] | None | Unset): The names the key may call: `provider/model` of a model in the
            catalog, or the name of a route. Left out, the key has no allowlist.
        expires_at (None | str | Unset):
        guardrail_ids (list[int] | None | Unset): The guardrails applied to every call of the key, in this order, after
            the gateway-wide ones and the route's. Admins only: sending the field
            at all, even `[]`, is refused for anyone else. At most 20.
        owner_id (int | None | Unset):
        tags (CreateKeyRequestTagsType0 | None | Unset): Tags every call of the key is recorded with, over those the
            call
            sends. At most 20; names of `A-Z a-z 0-9 _ . -`, names and values
            of 1 to 64 characters.
        team_id (int | None | Unset):
    """

    name: str
    allowed: list[str] | None | Unset = UNSET
    expires_at: None | str | Unset = UNSET
    guardrail_ids: list[int] | None | Unset = UNSET
    owner_id: int | None | Unset = UNSET
    tags: CreateKeyRequestTagsType0 | None | Unset = UNSET
    team_id: int | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        from ..models.create_key_request_tags_type_0 import (
            CreateKeyRequestTagsType0,
        )

        name = self.name

        allowed: list[str] | None | Unset
        if isinstance(self.allowed, Unset):
            allowed = UNSET
        elif isinstance(self.allowed, list):
            allowed = self.allowed

        else:
            allowed = self.allowed

        expires_at: None | str | Unset
        if isinstance(self.expires_at, Unset):
            expires_at = UNSET
        else:
            expires_at = self.expires_at

        guardrail_ids: list[int] | None | Unset
        if isinstance(self.guardrail_ids, Unset):
            guardrail_ids = UNSET
        elif isinstance(self.guardrail_ids, list):
            guardrail_ids = self.guardrail_ids

        else:
            guardrail_ids = self.guardrail_ids

        owner_id: int | None | Unset
        if isinstance(self.owner_id, Unset):
            owner_id = UNSET
        else:
            owner_id = self.owner_id

        tags: dict[str, Any] | None | Unset
        if isinstance(self.tags, Unset):
            tags = UNSET
        elif isinstance(self.tags, CreateKeyRequestTagsType0):
            tags = self.tags.to_dict()
        else:
            tags = self.tags

        team_id: int | None | Unset
        if isinstance(self.team_id, Unset):
            team_id = UNSET
        else:
            team_id = self.team_id

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "name": name,
            }
        )
        if allowed is not UNSET:
            field_dict["allowed"] = allowed
        if expires_at is not UNSET:
            field_dict["expires_at"] = expires_at
        if guardrail_ids is not UNSET:
            field_dict["guardrail_ids"] = guardrail_ids
        if owner_id is not UNSET:
            field_dict["owner_id"] = owner_id
        if tags is not UNSET:
            field_dict["tags"] = tags
        if team_id is not UNSET:
            field_dict["team_id"] = team_id

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.create_key_request_tags_type_0 import (
            CreateKeyRequestTagsType0,
        )

        d = dict(src_dict)
        name = d.pop("name")

        def _parse_allowed(data: object) -> list[str] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                allowed_type_0 = cast(list[str], data)

                return allowed_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None | Unset, data)

        allowed = _parse_allowed(d.pop("allowed", UNSET))

        def _parse_expires_at(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        expires_at = _parse_expires_at(d.pop("expires_at", UNSET))

        def _parse_guardrail_ids(data: object) -> list[int] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                guardrail_ids_type_0 = cast(list[int], data)

                return guardrail_ids_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[int] | None | Unset, data)

        guardrail_ids = _parse_guardrail_ids(d.pop("guardrail_ids", UNSET))

        def _parse_owner_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        owner_id = _parse_owner_id(d.pop("owner_id", UNSET))

        def _parse_tags(data: object) -> CreateKeyRequestTagsType0 | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                tags_type_0 = CreateKeyRequestTagsType0.from_dict(data)

                return tags_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(CreateKeyRequestTagsType0 | None | Unset, data)

        tags = _parse_tags(d.pop("tags", UNSET))

        def _parse_team_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        team_id = _parse_team_id(d.pop("team_id", UNSET))

        create_key_request = cls(
            name=name,
            allowed=allowed,
            expires_at=expires_at,
            guardrail_ids=guardrail_ids,
            owner_id=owner_id,
            tags=tags,
            team_id=team_id,
        )

        return create_key_request
