"""Results. Immutable, comparable, and holding nothing secret."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import List, Optional, Union


@dataclass(frozen=True)
class Message:
    """One chat message; `role` is "system", "user" or "assistant"."""

    role: str
    content: str


@dataclass(frozen=True)
class Usage:
    input_tokens: int
    output_tokens: int


@dataclass(frozen=True)
class ToolCall:
    """A tool call the model made; `arguments` is the JSON text it produced."""

    id: str
    name: str
    arguments: str


@dataclass(frozen=True)
class ChatResponse:
    id: str
    model: str
    content: str
    finish_reason: Optional[str]  # "stop", "length", "tool_calls", "content_filter"
    usage: Optional[Usage]
    tool_calls: List[ToolCall] = field(default_factory=list)


@dataclass(frozen=True)
class Delta:
    """A piece of the answer's text."""

    text: str


@dataclass(frozen=True)
class Done:
    """The last event of a complete stream."""

    finish_reason: Optional[str]
    usage: Optional[Usage]


@dataclass(frozen=True)
class ToolCallStart:
    """A tool call begins; `index` counts the answer's tool calls from 0."""

    index: int
    id: str
    name: str


@dataclass(frozen=True)
class ToolCallDelta:
    """More argument text for the call at `index`."""

    index: int
    arguments: str


StreamEvent = Union[Delta, ToolCallStart, ToolCallDelta, Done]


@dataclass(frozen=True)
class EmbeddingsResponse:
    model: str
    vectors: List[List[float]]
    prompt_tokens: int  # 0 when the provider does not report it
