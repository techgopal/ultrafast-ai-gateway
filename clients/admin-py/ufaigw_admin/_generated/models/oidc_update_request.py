from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="OidcUpdateRequest")


@_attrs_define
class OidcUpdateRequest:
    """Every setting. A field left out takes its default, except
    `client_secret`: left out, the stored secret is kept.

        Attributes:
            admin_group (str | Unset):
            allowed_domains (list[str] | Unset): At most 100 domains; stored in lower case.
            auto_create (bool | Unset): Needs at least one allowed domain. Default false.
            client_id (str | Unset): At most 512 bytes. May be empty while single sign-on is off.
            client_secret (str | Unset): Write only: replaces the stored secret. Left out: the stored secret
                stays. At most 4096 bytes.
            enabled (bool | Unset): Needs `UF_PUBLIC_URL`, an issuer, a client id and a client secret.
            groups_claim (str | Unset): Default "groups".
            issuer (str | Unset): An `https` URL (`http` only for localhost, 127.0.0.1 and [::1]),
                without credentials, query or fragment. May be empty while single
                sign-on is off.
            label (str | Unset): 1 to 40 characters. Default "SSO".
            link_by_email (bool | Unset): Default true.
            scopes (str | Unset): Extra scopes, space separated.
    """

    admin_group: str | Unset = UNSET
    allowed_domains: list[str] | Unset = UNSET
    auto_create: bool | Unset = UNSET
    client_id: str | Unset = UNSET
    client_secret: str | Unset = UNSET
    enabled: bool | Unset = UNSET
    groups_claim: str | Unset = UNSET
    issuer: str | Unset = UNSET
    label: str | Unset = UNSET
    link_by_email: bool | Unset = UNSET
    scopes: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        admin_group = self.admin_group

        allowed_domains: list[str] | Unset = UNSET
        if not isinstance(self.allowed_domains, Unset):
            allowed_domains = self.allowed_domains

        auto_create = self.auto_create

        client_id = self.client_id

        client_secret = self.client_secret

        enabled = self.enabled

        groups_claim = self.groups_claim

        issuer = self.issuer

        label = self.label

        link_by_email = self.link_by_email

        scopes = self.scopes

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if admin_group is not UNSET:
            field_dict["admin_group"] = admin_group
        if allowed_domains is not UNSET:
            field_dict["allowed_domains"] = allowed_domains
        if auto_create is not UNSET:
            field_dict["auto_create"] = auto_create
        if client_id is not UNSET:
            field_dict["client_id"] = client_id
        if client_secret is not UNSET:
            field_dict["client_secret"] = client_secret
        if enabled is not UNSET:
            field_dict["enabled"] = enabled
        if groups_claim is not UNSET:
            field_dict["groups_claim"] = groups_claim
        if issuer is not UNSET:
            field_dict["issuer"] = issuer
        if label is not UNSET:
            field_dict["label"] = label
        if link_by_email is not UNSET:
            field_dict["link_by_email"] = link_by_email
        if scopes is not UNSET:
            field_dict["scopes"] = scopes

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        admin_group = d.pop("admin_group", UNSET)

        allowed_domains = cast(list[str], d.pop("allowed_domains", UNSET))

        auto_create = d.pop("auto_create", UNSET)

        client_id = d.pop("client_id", UNSET)

        client_secret = d.pop("client_secret", UNSET)

        enabled = d.pop("enabled", UNSET)

        groups_claim = d.pop("groups_claim", UNSET)

        issuer = d.pop("issuer", UNSET)

        label = d.pop("label", UNSET)

        link_by_email = d.pop("link_by_email", UNSET)

        scopes = d.pop("scopes", UNSET)

        oidc_update_request = cls(
            admin_group=admin_group,
            allowed_domains=allowed_domains,
            auto_create=auto_create,
            client_id=client_id,
            client_secret=client_secret,
            enabled=enabled,
            groups_claim=groups_claim,
            issuer=issuer,
            label=label,
            link_by_email=link_by_email,
            scopes=scopes,
        )

        return oidc_update_request
