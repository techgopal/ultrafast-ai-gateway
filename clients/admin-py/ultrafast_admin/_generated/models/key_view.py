from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.guardrail_ref import GuardrailRef
    from ..models.key_view_tags import KeyViewTags


T = TypeVar("T", bound="KeyView")


@_attrs_define
class KeyView:
    """A key as `/api` shows it. It has no field for the key or its hash.

    Attributes:
        allowed (list[str] | None): The models and routes the key may call; `null` is no limit.
        created_at (str):
        display (str):
        expires_at (None | str):
        guardrails (list[GuardrailRef]): The guardrails applied to every call of the key, in order. Anyone who
            may see the key sees them; only an admin changes them.
        id (int):
        name (str):
        owner_email (None | str):
        owner_id (int | None):
        revoked_at (None | str):
        status (str): `revoked`, `expired`, `suspended` or `active`, the first that
            applies. `suspended`: the owner of the key is not active, so the key
            does not work until they are. Only an `active` key works.
        tags (KeyViewTags): Added to every call of the key; the key's value wins over the call's
            for the same name. Empty when none.
        team_id (int | None):
        team_name (None | str):
        team_only (bool): A non-admin made it for another user: it calls only what everyone
            or its team may use, and is revoked when its owner is deleted.
    """

    allowed: list[str] | None
    created_at: str
    display: str
    expires_at: None | str
    guardrails: list[GuardrailRef]
    id: int
    name: str
    owner_email: None | str
    owner_id: int | None
    revoked_at: None | str
    status: str
    tags: KeyViewTags
    team_id: int | None
    team_name: None | str
    team_only: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        allowed: list[str] | None
        if isinstance(self.allowed, list):
            allowed = self.allowed

        else:
            allowed = self.allowed

        created_at = self.created_at

        display = self.display

        expires_at: None | str
        expires_at = self.expires_at

        guardrails = []
        for guardrails_item_data in self.guardrails:
            guardrails_item = guardrails_item_data.to_dict()
            guardrails.append(guardrails_item)

        id = self.id

        name = self.name

        owner_email: None | str
        owner_email = self.owner_email

        owner_id: int | None
        owner_id = self.owner_id

        revoked_at: None | str
        revoked_at = self.revoked_at

        status = self.status

        tags = self.tags.to_dict()

        team_id: int | None
        team_id = self.team_id

        team_name: None | str
        team_name = self.team_name

        team_only = self.team_only

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "allowed": allowed,
                "created_at": created_at,
                "display": display,
                "expires_at": expires_at,
                "guardrails": guardrails,
                "id": id,
                "name": name,
                "owner_email": owner_email,
                "owner_id": owner_id,
                "revoked_at": revoked_at,
                "status": status,
                "tags": tags,
                "team_id": team_id,
                "team_name": team_name,
                "team_only": team_only,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.guardrail_ref import GuardrailRef
        from ..models.key_view_tags import KeyViewTags

        d = dict(src_dict)

        def _parse_allowed(data: object) -> list[str] | None:
            if data is None:
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                allowed_type_0 = cast(list[str], data)

                return allowed_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None, data)

        allowed = _parse_allowed(d.pop("allowed"))

        created_at = d.pop("created_at")

        display = d.pop("display")

        def _parse_expires_at(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        expires_at = _parse_expires_at(d.pop("expires_at"))

        guardrails = []
        _guardrails = d.pop("guardrails")
        for guardrails_item_data in _guardrails:
            guardrails_item = GuardrailRef.from_dict(guardrails_item_data)

            guardrails.append(guardrails_item)

        id = d.pop("id")

        name = d.pop("name")

        def _parse_owner_email(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        owner_email = _parse_owner_email(d.pop("owner_email"))

        def _parse_owner_id(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        owner_id = _parse_owner_id(d.pop("owner_id"))

        def _parse_revoked_at(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        revoked_at = _parse_revoked_at(d.pop("revoked_at"))

        status = d.pop("status")

        tags = KeyViewTags.from_dict(d.pop("tags"))

        def _parse_team_id(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        team_id = _parse_team_id(d.pop("team_id"))

        def _parse_team_name(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        team_name = _parse_team_name(d.pop("team_name"))

        team_only = d.pop("team_only")

        key_view = cls(
            allowed=allowed,
            created_at=created_at,
            display=display,
            expires_at=expires_at,
            guardrails=guardrails,
            id=id,
            name=name,
            owner_email=owner_email,
            owner_id=owner_id,
            revoked_at=revoked_at,
            status=status,
            tags=tags,
            team_id=team_id,
            team_name=team_name,
            team_only=team_only,
        )

        key_view.additional_properties = d
        return key_view

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
