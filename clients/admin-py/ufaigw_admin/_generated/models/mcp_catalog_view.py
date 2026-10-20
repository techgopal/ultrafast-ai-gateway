from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

from ..models.source import Source

if TYPE_CHECKING:
    from ..models.entry import Entry


T = TypeVar("T", bound="McpCatalogView")


@_attrs_define
class McpCatalogView:
    """The catalog of MCP servers the project publishes, as the console shows it.

    Attributes:
        entries (list[Entry]):
        error (None | str): Why the newest attempt did not give a newer copy, in our words.
        fetch_enabled (bool): Whether the gateway fetches the catalog at all (`UF_MCP_CATALOG`).
        fetched_at (None | str):
        local_enabled (bool): Whether servers that run as programs on this host may be added
            (`UF_MCP_LOCAL=on`).
        source (Source): Where the entries a view shows came from.
    """

    entries: list[Entry]
    error: None | str
    fetch_enabled: bool
    fetched_at: None | str
    local_enabled: bool
    source: Source
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        entries = []
        for entries_item_data in self.entries:
            entries_item = entries_item_data.to_dict()
            entries.append(entries_item)

        error: None | str
        error = self.error

        fetch_enabled = self.fetch_enabled

        fetched_at: None | str
        fetched_at = self.fetched_at

        local_enabled = self.local_enabled

        source = self.source.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "entries": entries,
                "error": error,
                "fetch_enabled": fetch_enabled,
                "fetched_at": fetched_at,
                "local_enabled": local_enabled,
                "source": source,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.entry import Entry

        d = dict(src_dict)
        entries = []
        _entries = d.pop("entries")
        for entries_item_data in _entries:
            entries_item = Entry.from_dict(entries_item_data)

            entries.append(entries_item)

        def _parse_error(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        error = _parse_error(d.pop("error"))

        fetch_enabled = d.pop("fetch_enabled")

        def _parse_fetched_at(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        fetched_at = _parse_fetched_at(d.pop("fetched_at"))

        local_enabled = d.pop("local_enabled")

        source = Source(d.pop("source"))

        mcp_catalog_view = cls(
            entries=entries,
            error=error,
            fetch_enabled=fetch_enabled,
            fetched_at=fetched_at,
            local_enabled=local_enabled,
            source=source,
        )

        mcp_catalog_view.additional_properties = d
        return mcp_catalog_view

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
