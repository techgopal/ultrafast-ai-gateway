from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from typing_extensions import Self

if TYPE_CHECKING:
    from ..models.guardrail_log import GuardrailLog
    from ..models.log_view_tags import LogViewTags


T = TypeVar("T", bound="LogView")


@_attrs_define
class LogView:
    """One logged call, as `/api` shows it.

    Attributes:
        at (str): UTC, `YYYY-MM-DD HH:MM:SS`.
        cached (bool): Answered from the response cache: `cost_micros` is 0 and `priced`
            is true, and the tokens are those of the cached answer, so usage
            reports count them; no provider was called.
        cost_micros (int): Cost in millionths of a dollar; 0 when `priced` is false.
        duration_ms (int):
        endpoint (str):
        estimated (bool): The tokens and cost are an estimate: a stream that ended without the
            provider's report (the caller went away, or an error came after
            content was sent) is charged the input of the call and the streamed
            characters / 4, and `priced` stays true when the model has a price.
        guardrails (GuardrailLog | None):
        id (int):
        input_tokens (int | None):
        key_id (int | None):
        key_name (None | str): `null` when the key was deleted.
        model (None | str):
        output_tokens (int | None):
        priced (bool):
        prompt (None | str): The prompt template the call used, as `name@version`, or `null`.
            It is text: it stays when the template is deleted.
        provider (None | str): The provider that answered; null when nothing answered (the model is then null too).
        requested (str): The model or route name the caller asked for.
        status (int): What the caller was answered.
        stream (bool):
        tags (LogViewTags): The tags of the call: what it sent in `x-uf-tags` overlaid by its
            key's. Empty when none.
        team_id (int | None):
        team_name (None | str):
        user_email (None | str):
        user_id (int | None):
    """

    at: str
    cached: bool
    cost_micros: int
    duration_ms: int
    endpoint: str
    estimated: bool
    guardrails: GuardrailLog | None
    id: int
    input_tokens: int | None
    key_id: int | None
    key_name: None | str
    model: None | str
    output_tokens: int | None
    priced: bool
    prompt: None | str
    provider: None | str
    requested: str
    status: int
    stream: bool
    tags: LogViewTags
    team_id: int | None
    team_name: None | str
    user_email: None | str
    user_id: int | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.guardrail_log import GuardrailLog

        at = self.at

        cached = self.cached

        cost_micros = self.cost_micros

        duration_ms = self.duration_ms

        endpoint = self.endpoint

        estimated = self.estimated

        guardrails: dict[str, Any] | None
        if isinstance(self.guardrails, GuardrailLog):
            guardrails = self.guardrails.to_dict()
        else:
            guardrails = self.guardrails

        id = self.id

        input_tokens: int | None
        input_tokens = self.input_tokens

        key_id: int | None
        key_id = self.key_id

        key_name: None | str
        key_name = self.key_name

        model: None | str
        model = self.model

        output_tokens: int | None
        output_tokens = self.output_tokens

        priced = self.priced

        prompt: None | str
        prompt = self.prompt

        provider: None | str
        provider = self.provider

        requested = self.requested

        status = self.status

        stream = self.stream

        tags = self.tags.to_dict()

        team_id: int | None
        team_id = self.team_id

        team_name: None | str
        team_name = self.team_name

        user_email: None | str
        user_email = self.user_email

        user_id: int | None
        user_id = self.user_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "at": at,
                "cached": cached,
                "cost_micros": cost_micros,
                "duration_ms": duration_ms,
                "endpoint": endpoint,
                "estimated": estimated,
                "guardrails": guardrails,
                "id": id,
                "input_tokens": input_tokens,
                "key_id": key_id,
                "key_name": key_name,
                "model": model,
                "output_tokens": output_tokens,
                "priced": priced,
                "prompt": prompt,
                "provider": provider,
                "requested": requested,
                "status": status,
                "stream": stream,
                "tags": tags,
                "team_id": team_id,
                "team_name": team_name,
                "user_email": user_email,
                "user_id": user_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.guardrail_log import GuardrailLog
        from ..models.log_view_tags import LogViewTags

        d = dict(src_dict)
        at = d.pop("at")

        cached = d.pop("cached")

        cost_micros = d.pop("cost_micros")

        duration_ms = d.pop("duration_ms")

        endpoint = d.pop("endpoint")

        estimated = d.pop("estimated")

        def _parse_guardrails(data: object) -> GuardrailLog | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                guardrails_type_0 = GuardrailLog.from_dict(data)

                return guardrails_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(GuardrailLog | None, data)

        guardrails = _parse_guardrails(d.pop("guardrails"))

        id = d.pop("id")

        def _parse_input_tokens(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        input_tokens = _parse_input_tokens(d.pop("input_tokens"))

        def _parse_key_id(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        key_id = _parse_key_id(d.pop("key_id"))

        def _parse_key_name(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        key_name = _parse_key_name(d.pop("key_name"))

        def _parse_model(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        model = _parse_model(d.pop("model"))

        def _parse_output_tokens(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        output_tokens = _parse_output_tokens(d.pop("output_tokens"))

        priced = d.pop("priced")

        def _parse_prompt(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        prompt = _parse_prompt(d.pop("prompt"))

        def _parse_provider(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        provider = _parse_provider(d.pop("provider"))

        requested = d.pop("requested")

        status = d.pop("status")

        stream = d.pop("stream")

        tags = LogViewTags.from_dict(d.pop("tags"))

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

        def _parse_user_email(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        user_email = _parse_user_email(d.pop("user_email"))

        def _parse_user_id(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        user_id = _parse_user_id(d.pop("user_id"))

        log_view = cls(
            at=at,
            cached=cached,
            cost_micros=cost_micros,
            duration_ms=duration_ms,
            endpoint=endpoint,
            estimated=estimated,
            guardrails=guardrails,
            id=id,
            input_tokens=input_tokens,
            key_id=key_id,
            key_name=key_name,
            model=model,
            output_tokens=output_tokens,
            priced=priced,
            prompt=prompt,
            provider=provider,
            requested=requested,
            status=status,
            stream=stream,
            tags=tags,
            team_id=team_id,
            team_name=team_name,
            user_email=user_email,
            user_id=user_id,
        )

        log_view.additional_properties = d
        return log_view

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
