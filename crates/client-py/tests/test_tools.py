import json

import pytest
import ultrafast
from conftest import ANTHROPIC_CHAT, KEY, OPENAI_CHAT, Script, sse

TOOL_ANSWER = json.dumps(
    {
        "id": "c2",
        "model": "gpt-4o",
        "choices": [
            {
                "message": {
                    "role": "assistant",
                    "content": None,
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "weather", "arguments": '{"city":"Paris"}'},
                        }
                    ],
                },
                "finish_reason": "tool_calls",
            }
        ],
        "usage": {"prompt_tokens": 9, "completion_tokens": 4},
    }
)

WEATHER = {
    "type": "function",
    "function": {
        "name": "weather",
        "description": "Current weather",
        "parameters": {"type": "object", "properties": {"city": {"type": "string"}}},
    },
}

CONVERSATION = [
    {
        "role": "user",
        "content": [
            {"type": "text", "text": "weather where this was taken?"},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
        ],
    },
    {
        "role": "assistant",
        "content": None,
        "tool_calls": [
            {
                "id": "call_1",
                "type": "function",
                "function": {"name": "weather", "arguments": '{"city":"Paris"}'},
            }
        ],
    },
    {"role": "tool", "tool_call_id": "call_1", "content": "sunny"},
]


def delta(d, finish=None):
    return json.dumps({"choices": [{"delta": d, "finish_reason": finish}]})


TOOL_STREAM = sse(
    delta({"tool_calls": [{"index": 0, "id": "call_1", "function": {"name": "weather", "arguments": ""}}]}),
    delta({"tool_calls": [{"index": 0, "function": {"arguments": '{"city":'}}]}),
    delta({"tool_calls": [{"index": 0, "function": {"arguments": '"Paris"}'}}]}),
    delta({}, "tool_calls"),
    json.dumps({"choices": [], "usage": {"prompt_tokens": 9, "completion_tokens": 4}}),
    "[DONE]",
)

EXPECTED_EVENTS = [
    ultrafast.ToolCallStart(0, "call_1", "weather"),
    ultrafast.ToolCallDelta(0, '{"city":'),
    ultrafast.ToolCallDelta(0, '"Paris"}'),
    ultrafast.Done("tool_calls", ultrafast.Usage(9, 4)),
]


def test_chat_sends_tools_images_and_results_and_returns_tool_calls(serve):
    s = serve(Script.json(200, TOOL_ANSWER))
    c = ultrafast.Client(ultrafast.gateway(s.url, KEY))
    r = c.chat(
        "gpt-4o",
        CONVERSATION,
        tools=[WEATHER],
        tool_choice="weather",
        parallel_tool_calls=False,
    )
    assert r.finish_reason == "tool_calls"
    assert r.content == ""
    assert r.tool_calls == [ultrafast.ToolCall("call_1", "weather", '{"city":"Paris"}')]
    body = json.loads(s.only()["body"])
    assert body["messages"][0]["content"][1] == {
        "type": "image_url",
        "image_url": {"url": "data:image/png;base64,AAAA"},
    }
    assert body["messages"][1]["tool_calls"][0]["function"]["name"] == "weather"
    assert body["messages"][2]["tool_call_id"] == "call_1"
    assert body["tools"][0]["function"]["name"] == "weather"
    assert body["tool_choice"] == {"type": "function", "function": {"name": "weather"}}
    assert body["parallel_tool_calls"] is False


@pytest.mark.parametrize("choice", ["auto", "none", "required"])
def test_tool_choice_words(serve, choice):
    s = serve(Script.json(200, OPENAI_CHAT))
    ultrafast.Client(ultrafast.gateway(s.url, KEY)).chat(
        "m", [{"role": "user", "content": "x"}], tools=[WEATHER], tool_choice=choice
    )
    assert json.loads(s.only()["body"])["tool_choice"] == choice


def test_a_plain_answer_has_no_tool_calls(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    r = ultrafast.Client(ultrafast.gateway(s.url, KEY)).chat("m", [{"role": "user", "content": "x"}])
    assert r.tool_calls == []


def test_tuples_still_work_and_mix_with_dicts(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    ultrafast.Client(ultrafast.gateway(s.url, KEY)).chat(
        "m", [("system", "be brief"), {"role": "user", "content": "hi"}, ultrafast.Message("assistant", "ok")]
    )
    body = json.loads(s.only()["body"])
    assert [m["role"] for m in body["messages"]] == ["system", "user", "assistant"]


def test_a_direct_provider_target_gets_tools_and_images_too(serve):
    s = serve(Script.json(200, ANTHROPIC_CHAT))
    c = ultrafast.Client(ultrafast.anthropic("ak-1", s.url))
    c.chat("claude-sonnet-5", [CONVERSATION[0]], tools=[WEATHER])
    body = json.loads(s.only()["body"])
    assert body["tools"][0]["name"] == "weather"
    assert body["messages"][0]["content"][1]["type"] == "image"


def test_stream_has_tool_call_events(serve):
    s = serve(Script.sse(TOOL_STREAM))
    c = ultrafast.Client(ultrafast.gateway(s.url, KEY))
    assert list(c.chat_stream("m", CONVERSATION, tools=[WEATHER])) == EXPECTED_EVENTS


@pytest.mark.asyncio
async def test_async_chat_and_stream(serve):
    s = serve(Script.json(200, TOOL_ANSWER))
    c = ultrafast.AsyncClient(ultrafast.gateway(s.url, KEY))
    r = await c.chat("m", CONVERSATION, tools=[WEATHER])
    assert r.tool_calls[0].name == "weather"
    s2 = serve(Script.sse(TOOL_STREAM))
    c2 = ultrafast.AsyncClient(ultrafast.gateway(s2.url, KEY))
    assert [e async for e in c2.chat_stream("m", CONVERSATION, tools=[WEATHER])] == EXPECTED_EVENTS


def test_bad_input_is_refused_before_sending(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    c = ultrafast.Client(ultrafast.gateway(s.url, KEY))
    user = {"role": "user", "content": "x"}
    for messages in (
        [{"role": "tool", "content": "sunny"}],  # no tool_call_id
        [{"role": "user", "content": [{"type": "image_url", "image_url": {"url": "ftp://x/a.png"}}]}],
        [{"role": "user", "content": [{"type": "audio"}]}],
        [{"role": "robot", "content": "x"}],
        [{"role": "user", "content": None}],
    ):
        with pytest.raises((ultrafast.InvalidRequestError, ValueError)):
            c.chat("m", messages)
    with pytest.raises(ultrafast.InvalidRequestError):
        c.chat("m", [{"role": "tool", "content": "sunny"}])
    with pytest.raises(ultrafast.InvalidRequestError):
        c.chat("m", [user], tools=[{"type": "function", "function": {"name": ""}}])
    with pytest.raises(TypeError):
        c.chat("m", [user], tools=[WEATHER], tool_choice=42)
    with pytest.raises(TypeError):
        c.chat("m", [user], tools="weather")
    assert s.requests == []


FLAT_WEATHER = {
    "name": "weather",
    "description": "Current weather",
    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}},
}


def test_the_natural_tool_loop_round_trips(serve):
    s = serve(Script.json(200, TOOL_ANSWER))
    c = ultrafast.Client(ultrafast.gateway(s.url, KEY))
    messages = [{"role": "user", "content": "weather in Paris?"}]
    reply = c.chat("m", messages, tools=[FLAT_WEATHER])
    messages.append({"role": "assistant", "content": reply.content, "tool_calls": reply.tool_calls})
    messages.append({"role": "tool", "tool_call_id": reply.tool_calls[0].id, "content": "sunny"})
    s2 = serve(Script.json(200, OPENAI_CHAT))
    ultrafast.Client(ultrafast.gateway(s2.url, KEY)).chat("m", messages, tools=[FLAT_WEATHER])
    body = json.loads(s2.only()["body"])
    assert body["messages"][1]["tool_calls"] == [
        {"id": "call_1", "type": "function", "function": {"name": "weather", "arguments": '{"city":"Paris"}'}}
    ]
    assert body["messages"][2]["tool_call_id"] == "call_1"


def test_flat_dict_tool_calls_and_message_objects_carry_tools(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    ultrafast.Client(ultrafast.gateway(s.url, KEY)).chat(
        "m",
        [
            ultrafast.Message("user", [{"type": "text", "text": "hi"}]),
            ultrafast.Message(
                "assistant", None, tool_calls=[{"id": "c1", "name": "weather", "arguments": "{}"}]
            ),
            ultrafast.Message("tool", "sunny", tool_call_id="c1"),
        ],
    )
    body = json.loads(s.only()["body"])
    assert body["messages"][1]["tool_calls"][0]["function"]["name"] == "weather"
    assert body["messages"][2]["tool_call_id"] == "c1"


def test_flat_and_openai_tool_shapes_send_the_same_body(serve):
    bodies = []
    for tool in (FLAT_WEATHER, WEATHER):
        s = serve(Script.json(200, OPENAI_CHAT))
        ultrafast.Client(ultrafast.gateway(s.url, KEY)).chat(
            "m", [{"role": "user", "content": "x"}], tools=[tool]
        )
        bodies.append(json.loads(s.only()["body"])["tools"])
    assert bodies[0] == bodies[1]
    assert bodies[0][0]["function"]["name"] == "weather"


def test_strict_is_carried_in_both_tool_shapes(serve):
    for tool in (
        {**FLAT_WEATHER, "strict": True},
        {"type": "function", "function": {**WEATHER["function"], "strict": True}},
    ):
        s = serve(Script.json(200, OPENAI_CHAT))
        ultrafast.Client(ultrafast.gateway(s.url, KEY)).chat(
            "m", [{"role": "user", "content": "x"}], tools=[tool, FLAT_WEATHER | {"name": "b"}]
        )
        tools = json.loads(s.only()["body"])["tools"]
        assert tools[0]["function"]["strict"] is True
        assert "strict" not in tools[1]["function"]


SCHEMA_FORMAT = {
    "type": "json_schema",
    "json_schema": {
        "name": "pet",
        "schema": {"type": "object", "properties": {"n": {"type": "string"}}},
        "strict": True,
    },
}
MSG = [{"role": "user", "content": "x"}]


def test_response_format_reaches_a_gateway_as_given(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    ultrafast.Client(ultrafast.gateway(s.url, KEY)).chat("m", MSG, response_format=SCHEMA_FORMAT)
    assert json.loads(s.only()["body"])["response_format"] == SCHEMA_FORMAT


def test_response_format_json_object_and_absent(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    c = ultrafast.Client(ultrafast.gateway(s.url, KEY))
    c.chat("m", MSG, response_format={"type": "json_object"})
    c.chat("m", MSG)
    bodies = [json.loads(r["body"]) for r in s.requests]
    assert bodies[0]["response_format"] == {"type": "json_object"}
    assert "response_format" not in bodies[1]


def test_response_format_uses_output_config_for_anthropic(serve):
    s = serve(Script.json(200, ANTHROPIC_CHAT))
    ultrafast.Client(ultrafast.anthropic(KEY, s.url)).chat(
        "claude-sonnet-5", MSG, response_format=SCHEMA_FORMAT
    )
    body = json.loads(s.only()["body"])
    assert body["output_config"] == {
        "format": {"type": "json_schema", "schema": SCHEMA_FORMAT["json_schema"]["schema"]}
    }


def test_a_bad_response_format_is_refused_before_sending(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    c = ultrafast.Client(ultrafast.gateway(s.url, KEY))
    with pytest.raises(TypeError):
        c.chat("m", MSG, response_format="json")
    with pytest.raises(ultrafast.InvalidRequestError):
        c.chat("m", MSG, response_format={"type": "xml"})
    assert s.requests == []
