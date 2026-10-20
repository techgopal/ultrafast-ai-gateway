from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..types import UNSET, Unset

T = TypeVar("T", bound="OidcTestResult")


@_attrs_define
class OidcTestResult:
    """
    Attributes:
        ok (bool): Whether the discovery document and the key set could be used.
        authorization_endpoint (None | str | Unset):
        error (None | str | Unset): What is wrong, when `ok` is false. It never repeats a URL.
        issuer (None | str | Unset): The issuer the provider named, when its discovery document was read.
        jwks_keys (int | None | Unset): How many keys the provider publishes to verify ID tokens.
        token_endpoint (None | str | Unset):
    """

    ok: bool
    authorization_endpoint: None | str | Unset = UNSET
    error: None | str | Unset = UNSET
    issuer: None | str | Unset = UNSET
    jwks_keys: int | None | Unset = UNSET
    token_endpoint: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        ok = self.ok

        authorization_endpoint: None | str | Unset
        if isinstance(self.authorization_endpoint, Unset):
            authorization_endpoint = UNSET
        else:
            authorization_endpoint = self.authorization_endpoint

        error: None | str | Unset
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        issuer: None | str | Unset
        if isinstance(self.issuer, Unset):
            issuer = UNSET
        else:
            issuer = self.issuer

        jwks_keys: int | None | Unset
        if isinstance(self.jwks_keys, Unset):
            jwks_keys = UNSET
        else:
            jwks_keys = self.jwks_keys

        token_endpoint: None | str | Unset
        if isinstance(self.token_endpoint, Unset):
            token_endpoint = UNSET
        else:
            token_endpoint = self.token_endpoint

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "ok": ok,
            }
        )
        if authorization_endpoint is not UNSET:
            field_dict["authorization_endpoint"] = authorization_endpoint
        if error is not UNSET:
            field_dict["error"] = error
        if issuer is not UNSET:
            field_dict["issuer"] = issuer
        if jwks_keys is not UNSET:
            field_dict["jwks_keys"] = jwks_keys
        if token_endpoint is not UNSET:
            field_dict["token_endpoint"] = token_endpoint

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        ok = d.pop("ok")

        def _parse_authorization_endpoint(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        authorization_endpoint = _parse_authorization_endpoint(
            d.pop("authorization_endpoint", UNSET)
        )

        def _parse_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error = _parse_error(d.pop("error", UNSET))

        def _parse_issuer(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        issuer = _parse_issuer(d.pop("issuer", UNSET))

        def _parse_jwks_keys(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        jwks_keys = _parse_jwks_keys(d.pop("jwks_keys", UNSET))

        def _parse_token_endpoint(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        token_endpoint = _parse_token_endpoint(d.pop("token_endpoint", UNSET))

        oidc_test_result = cls(
            ok=ok,
            authorization_endpoint=authorization_endpoint,
            error=error,
            issuer=issuer,
            jwks_keys=jwks_keys,
            token_endpoint=token_endpoint,
        )

        oidc_test_result.additional_properties = d
        return oidc_test_result

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
