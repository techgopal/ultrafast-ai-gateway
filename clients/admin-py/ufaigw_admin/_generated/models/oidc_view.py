from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="OidcView")


@_attrs_define
class OidcView:
    """
    Attributes:
        admin_group (str): Members of this group become admins on sign-in; empty: roles are
            never changed by sign-in.
        allowed_domains (list[str]): Lower case domains, like `example.com`.
        auto_create (bool): Whether a person from an allowed domain gets a Member account on
            first sign-in.
        client_id (str):
        client_secret_set (bool): Whether a client secret is stored. The secret itself is never
            returned.
        client_secret_unreadable (bool): True when a secret is stored but cannot be decrypted (the master key
            changed). Single sign-on stays off until a new secret is saved.
        enabled (bool): Whether the sign-in button is offered.
        groups_claim (str): The ID token claim that lists the user's groups.
        issuer (str): The issuer URL of the identity provider; empty until set.
        label (str): The name on the button: "Sign in with <label>".
        link_by_email (bool): Whether a person who signs in is matched to an existing user by a
            verified email address.
        public_url_set (bool): Whether the gateway was started with `UF_PUBLIC_URL`. Single
            sign-on cannot be turned on without it.
        scopes (str): Scopes asked for besides `openid email profile`, space separated.
        redirect_uri (None | str | Unset): The address to register at the identity provider; null without a
            public URL.
    """

    admin_group: str
    allowed_domains: list[str]
    auto_create: bool
    client_id: str
    client_secret_set: bool
    client_secret_unreadable: bool
    enabled: bool
    groups_claim: str
    issuer: str
    label: str
    link_by_email: bool
    public_url_set: bool
    scopes: str
    redirect_uri: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        admin_group = self.admin_group

        allowed_domains = self.allowed_domains

        auto_create = self.auto_create

        client_id = self.client_id

        client_secret_set = self.client_secret_set

        client_secret_unreadable = self.client_secret_unreadable

        enabled = self.enabled

        groups_claim = self.groups_claim

        issuer = self.issuer

        label = self.label

        link_by_email = self.link_by_email

        public_url_set = self.public_url_set

        scopes = self.scopes

        redirect_uri: None | str | Unset
        if isinstance(self.redirect_uri, Unset):
            redirect_uri = UNSET
        else:
            redirect_uri = self.redirect_uri

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "admin_group": admin_group,
                "allowed_domains": allowed_domains,
                "auto_create": auto_create,
                "client_id": client_id,
                "client_secret_set": client_secret_set,
                "client_secret_unreadable": client_secret_unreadable,
                "enabled": enabled,
                "groups_claim": groups_claim,
                "issuer": issuer,
                "label": label,
                "link_by_email": link_by_email,
                "public_url_set": public_url_set,
                "scopes": scopes,
            }
        )
        if redirect_uri is not UNSET:
            field_dict["redirect_uri"] = redirect_uri

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        admin_group = d.pop("admin_group")

        allowed_domains = cast(list[str], d.pop("allowed_domains"))

        auto_create = d.pop("auto_create")

        client_id = d.pop("client_id")

        client_secret_set = d.pop("client_secret_set")

        client_secret_unreadable = d.pop("client_secret_unreadable")

        enabled = d.pop("enabled")

        groups_claim = d.pop("groups_claim")

        issuer = d.pop("issuer")

        label = d.pop("label")

        link_by_email = d.pop("link_by_email")

        public_url_set = d.pop("public_url_set")

        scopes = d.pop("scopes")

        def _parse_redirect_uri(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        redirect_uri = _parse_redirect_uri(d.pop("redirect_uri", UNSET))

        oidc_view = cls(
            admin_group=admin_group,
            allowed_domains=allowed_domains,
            auto_create=auto_create,
            client_id=client_id,
            client_secret_set=client_secret_set,
            client_secret_unreadable=client_secret_unreadable,
            enabled=enabled,
            groups_claim=groups_claim,
            issuer=issuer,
            label=label,
            link_by_email=link_by_email,
            public_url_set=public_url_set,
            scopes=scopes,
            redirect_uri=redirect_uri,
        )

        oidc_view.additional_properties = d
        return oidc_view

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
