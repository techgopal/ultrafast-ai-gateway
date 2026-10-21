from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.update_key_request_tags_type_0 import UpdateKeyRequestTagsType0


T = TypeVar("T", bound="UpdateKeyRequest")


@_attrs_define
class UpdateKeyRequest:
    """What to change on a key. Admins; `pinned_tools` also a lead of the key's
    team. Send at least one field.

        Attributes:
            guardrail_ids (list[int] | None | Unset): Replaces the guardrails of the key; `[]` takes them all off. Left
                out, they stay.
            pinned_tools (list[str] | None | Unset): Replaces the MCP tools pinned to the key (`mcp:<server>/<tool>`, at
                most 100, each one the key may call); `[]` unpins them all. The
                combined MCP endpoint lists them, with the search tools, when the key
                can call more tools than `UF_MCP_LIST_INLINE_MAX`. A lead of the key's
                team, a lead's own key, or an admin. Left out, they stay.
            tags (None | Unset | UpdateKeyRequestTagsType0): Replaces all the tags of the key; `{}` removes them. The same
                limits
                as when the key is created. Left out, the tags stay.
    """

    guardrail_ids: list[int] | None | Unset = UNSET
    pinned_tools: list[str] | None | Unset = UNSET
    tags: None | Unset | UpdateKeyRequestTagsType0 = UNSET

    def to_dict(self) -> dict[str, Any]:
        from ..models.update_key_request_tags_type_0 import (
            UpdateKeyRequestTagsType0,
        )

        guardrail_ids: list[int] | None | Unset
        if isinstance(self.guardrail_ids, Unset):
            guardrail_ids = UNSET
        elif isinstance(self.guardrail_ids, list):
            guardrail_ids = self.guardrail_ids

        else:
            guardrail_ids = self.guardrail_ids

        pinned_tools: list[str] | None | Unset
        if isinstance(self.pinned_tools, Unset):
            pinned_tools = UNSET
        elif isinstance(self.pinned_tools, list):
            pinned_tools = self.pinned_tools

        else:
            pinned_tools = self.pinned_tools

        tags: dict[str, Any] | None | Unset
        if isinstance(self.tags, Unset):
            tags = UNSET
        elif isinstance(self.tags, UpdateKeyRequestTagsType0):
            tags = self.tags.to_dict()
        else:
            tags = self.tags

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if guardrail_ids is not UNSET:
            field_dict["guardrail_ids"] = guardrail_ids
        if pinned_tools is not UNSET:
            field_dict["pinned_tools"] = pinned_tools
        if tags is not UNSET:
            field_dict["tags"] = tags

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.update_key_request_tags_type_0 import (
            UpdateKeyRequestTagsType0,
        )

        d = dict(src_dict)

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

        def _parse_pinned_tools(data: object) -> list[str] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                pinned_tools_type_0 = cast(list[str], data)

                return pinned_tools_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None | Unset, data)

        pinned_tools = _parse_pinned_tools(d.pop("pinned_tools", UNSET))

        def _parse_tags(data: object) -> None | Unset | UpdateKeyRequestTagsType0:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                tags_type_0 = UpdateKeyRequestTagsType0.from_dict(data)

                return tags_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UpdateKeyRequestTagsType0, data)

        tags = _parse_tags(d.pop("tags", UNSET))

        update_key_request = cls(
            guardrail_ids=guardrail_ids,
            pinned_tools=pinned_tools,
            tags=tags,
        )

        return update_key_request
