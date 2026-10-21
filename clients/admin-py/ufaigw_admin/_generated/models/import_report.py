from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.issue import Issue
    from ..models.item import Item


T = TypeVar("T", bound="ImportReport")


@_attrs_define
class ImportReport:
    """What an import did, or with `dry_run` would do.

    Attributes:
        created (list[Item]):
        errors (list[Issue]): With any error nothing is written.
        unchanged (int): How many things of the file were there already, as they are.
        updated (list[Item]):
        warnings (list[Issue]):
    """

    created: list[Item]
    errors: list[Issue]
    unchanged: int
    updated: list[Item]
    warnings: list[Issue]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        created = []
        for created_item_data in self.created:
            created_item = created_item_data.to_dict()
            created.append(created_item)

        errors = []
        for errors_item_data in self.errors:
            errors_item = errors_item_data.to_dict()
            errors.append(errors_item)

        unchanged = self.unchanged

        updated = []
        for updated_item_data in self.updated:
            updated_item = updated_item_data.to_dict()
            updated.append(updated_item)

        warnings = []
        for warnings_item_data in self.warnings:
            warnings_item = warnings_item_data.to_dict()
            warnings.append(warnings_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "created": created,
                "errors": errors,
                "unchanged": unchanged,
                "updated": updated,
                "warnings": warnings,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.issue import Issue
        from ..models.item import Item

        d = dict(src_dict)
        created = []
        _created = d.pop("created")
        for created_item_data in _created:
            created_item = Item.from_dict(created_item_data)

            created.append(created_item)

        errors = []
        _errors = d.pop("errors")
        for errors_item_data in _errors:
            errors_item = Issue.from_dict(errors_item_data)

            errors.append(errors_item)

        unchanged = d.pop("unchanged")

        updated = []
        _updated = d.pop("updated")
        for updated_item_data in _updated:
            updated_item = Item.from_dict(updated_item_data)

            updated.append(updated_item)

        warnings = []
        _warnings = d.pop("warnings")
        for warnings_item_data in _warnings:
            warnings_item = Issue.from_dict(warnings_item_data)

            warnings.append(warnings_item)

        import_report = cls(
            created=created,
            errors=errors,
            unchanged=unchanged,
            updated=updated,
            warnings=warnings,
        )

        import_report.additional_properties = d
        return import_report

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
