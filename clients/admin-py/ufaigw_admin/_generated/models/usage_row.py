from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

T = TypeVar("T", bound="UsageRow")


@_attrs_define
class UsageRow:
    """Sums over one group of calls.

    Attributes:
        cache_read_tokens (int): Of `input_tokens`: those read from the providers' prompt caches.
        cache_write_tokens (int): Of `input_tokens`: those written to the providers' prompt caches.
        cancelled (int): Calls the caller abandoned before they were answered (status 499): not
            errors of the gateway or of a provider.
        cost_micros (int): In millionths of a dollar.
        errors (int): Calls answered with a status of 400 or more, except 499.
        group (str): The day (`YYYY-MM-DD`), the model name, or the id of the key, user or
            team. Empty for calls that have no key, user or team. `total` in the
            total row.
        input_tokens (int): Every token sent in, including those read from or written to the
            providers' prompt caches, and the tokens of cached answers (`cached`
            rows of the logs), which cost nothing.
        label (str): What to show: the day, the model name, the key or team name, the
            user's email, `(none)` for calls without a key, user or team,
            `(deleted)` when that object is gone, `Total` in the total row.
        output_tokens (int):
        requests (int):
        unpriced_requests (int): Calls that reported token usage but could not be priced, so their
            cost is missing from `cost_micros`.
    """

    cache_read_tokens: int
    cache_write_tokens: int
    cancelled: int
    cost_micros: int
    errors: int
    group: str
    input_tokens: int
    label: str
    output_tokens: int
    requests: int
    unpriced_requests: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        cache_read_tokens = self.cache_read_tokens

        cache_write_tokens = self.cache_write_tokens

        cancelled = self.cancelled

        cost_micros = self.cost_micros

        errors = self.errors

        group = self.group

        input_tokens = self.input_tokens

        label = self.label

        output_tokens = self.output_tokens

        requests = self.requests

        unpriced_requests = self.unpriced_requests

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "cache_read_tokens": cache_read_tokens,
                "cache_write_tokens": cache_write_tokens,
                "cancelled": cancelled,
                "cost_micros": cost_micros,
                "errors": errors,
                "group": group,
                "input_tokens": input_tokens,
                "label": label,
                "output_tokens": output_tokens,
                "requests": requests,
                "unpriced_requests": unpriced_requests,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        cache_read_tokens = d.pop("cache_read_tokens")

        cache_write_tokens = d.pop("cache_write_tokens")

        cancelled = d.pop("cancelled")

        cost_micros = d.pop("cost_micros")

        errors = d.pop("errors")

        group = d.pop("group")

        input_tokens = d.pop("input_tokens")

        label = d.pop("label")

        output_tokens = d.pop("output_tokens")

        requests = d.pop("requests")

        unpriced_requests = d.pop("unpriced_requests")

        usage_row = cls(
            cache_read_tokens=cache_read_tokens,
            cache_write_tokens=cache_write_tokens,
            cancelled=cancelled,
            cost_micros=cost_micros,
            errors=errors,
            group=group,
            input_tokens=input_tokens,
            label=label,
            output_tokens=output_tokens,
            requests=requests,
            unpriced_requests=unpriced_requests,
        )

        usage_row.additional_properties = d
        return usage_row

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
