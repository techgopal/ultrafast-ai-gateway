from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.usage_row import UsageRow


T = TypeVar("T", bound="UsagePage")


@_attrs_define
class UsagePage:
    """
    Attributes:
        from_ (str): First day of the range, `YYYY-MM-DD`, UTC.
        rows (list[UsageRow]):
        to (str): Last day of the range, `YYYY-MM-DD`, UTC.
        total (UsageRow): Sums over one group of calls.
    """

    from_: str
    rows: list[UsageRow]
    to: str
    total: UsageRow
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from_ = self.from_

        rows = []
        for rows_item_data in self.rows:
            rows_item = rows_item_data.to_dict()
            rows.append(rows_item)

        to = self.to

        total = self.total.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "from": from_,
                "rows": rows,
                "to": to,
                "total": total,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.usage_row import UsageRow

        d = dict(src_dict)
        from_ = d.pop("from")

        rows = []
        _rows = d.pop("rows")
        for rows_item_data in _rows:
            rows_item = UsageRow.from_dict(rows_item_data)

            rows.append(rows_item)

        to = d.pop("to")

        total = UsageRow.from_dict(d.pop("total"))

        usage_page = cls(
            from_=from_,
            rows=rows,
            to=to,
            total=total,
        )

        usage_page.additional_properties = d
        return usage_page

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
