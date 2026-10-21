import json

import pytest
import ufaigw
from conftest import ANTHROPIC_CHAT, KEY, OPENAI_CHAT, Script

MSGS = [{"role": "user", "content": "hi"}]


def check(r):
    assert isinstance(r, ufaigw.ChatResponse)
    assert r.content == "hello"
    assert r.finish_reason == "stop"
    assert r.usage == ufaigw.Usage(input_tokens=3, output_tokens=2)


def test_gateway_chat_sends_the_openai_wire_format(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    c = ufaigw.Client(ufaigw.gateway(s.url, KEY))
    r = c.chat(
        "m", MSGS, max_tokens=7, temperature=0.5, top_p=0.9, stop=["x"], tags={"team": "search"}
    )
    check(r)
    req = s.only()
    assert req["path"] == "/v1/chat/completions"
    assert req["headers"]["authorization"] == f"Bearer {KEY}"
    assert req["headers"]["x-uf-tags"] == '{"team":"search"}'
    body = json.loads(req["body"])
    assert body["model"] == "m"
    assert body["messages"] == [{"role": "user", "content": "hi"}]
    assert body["max_tokens"] == 7 and body["top_p"] == pytest.approx(0.9)
    assert body["stop"] == ["x"]


def test_a_trailing_v1_on_the_gateway_url_is_accepted(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    check(ufaigw.Client(ufaigw.gateway(s.url + "/v1/", KEY)).chat("m", MSGS))
    assert s.only()["path"] == "/v1/chat/completions"


def test_openai_provider_with_a_base_url(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    check(ufaigw.Client(ufaigw.openai(KEY, base_url=s.url + "/v1")).chat("m", MSGS))
    assert s.only()["path"] == "/v1/chat/completions"


def test_other_provider_targets(serve):
    s = serve(Script.json(200, ANTHROPIC_CHAT))
    c = ufaigw.Client(ufaigw.anthropic(KEY, base_url=s.url))
    check(c.chat("claude", [{"role": "system", "content": "be brief"}] + MSGS))
    assert s.only()["headers"]["x-api-key"] == KEY

    s = serve(Script.json(200, OPENAI_CHAT))
    c = ufaigw.Client(ufaigw.azure(s.url, KEY, api_version="2024-06-01"))
    check(c.chat("dep", MSGS))
    req = s.only()
    assert req["path"].startswith("/openai/deployments/dep/chat/completions?")
    assert "api-version=2024-06-01" in req["path"]
    assert req["headers"]["api-key"] == KEY

    s = serve(Script.json(200, OPENAI_CHAT))
    check(ufaigw.Client(ufaigw.openai_compatible(s.url + "/v1", KEY)).chat("m", MSGS))
    assert s.only()["path"] == "/v1/chat/completions"


def test_tags_go_to_a_gateway_and_never_to_a_provider(serve):
    cases = [
        (lambda u: ufaigw.openai(KEY, base_url=u + "/v1"), OPENAI_CHAT),
        (lambda u: ufaigw.openai_compatible(u + "/v1", KEY), OPENAI_CHAT),
        (lambda u: ufaigw.anthropic(KEY, base_url=u), ANTHROPIC_CHAT),
        (lambda u: ufaigw.azure(u, KEY), OPENAI_CHAT),
    ]
    for make, body in cases:
        s = serve(Script.json(200, body))
        ufaigw.Client(make(s.url)).chat("m", MSGS, tags={"team": "search"})
        req = s.only()
        assert "x-uf-tags" not in req["headers"]
        assert "search" not in req["body"] and "search" not in req["path"]


def test_too_large_tags_are_refused_before_sending(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    c = ufaigw.Client(ufaigw.gateway(s.url, KEY))
    with pytest.raises(ufaigw.InvalidRequestError):
        c.chat("m", MSGS, tags={"k": "x" * 2000})
    assert s.requests == []


def test_bad_arguments_are_python_errors(serve):
    c = ufaigw.Client(ufaigw.gateway("http://127.0.0.1:1", KEY))
    with pytest.raises(ValueError):
        c.chat("m", [{"role": "robot", "content": "x"}])
    with pytest.raises(TypeError):
        c.chat("m", [{"role": "user", "content": 5}])
    with pytest.raises(TypeError):
        c.chat("m", MSGS, tags={"a": 1})


def test_a_message_object_and_a_single_stop_string(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    c = ufaigw.Client(ufaigw.gateway(s.url, KEY))
    check(c.chat("m", [ufaigw.Message("user", "hi")], stop="END"))
    assert json.loads(s.only()["body"])["stop"] == ["END"]


def test_result_objects_are_immutable_and_comparable():
    u = ufaigw.Usage(1, 2)
    with pytest.raises(Exception):
        u.input_tokens = 5
    assert u == ufaigw.Usage(1, 2)
    assert "ChatResponse(" in repr(ufaigw.ChatResponse("i", "m", "c", "stop", u))


def test_the_gil_is_released_during_io(serve):
    """A second Python thread keeps running while a call waits for the server."""
    import threading
    import time

    s = serve(Script.json(200, OPENAI_CHAT, ))
    s.script.hang = True
    c = ufaigw.Client(ufaigw.gateway(s.url, KEY), timeout=0.6)
    ticks = []
    stop = threading.Event()

    def tick():
        while not stop.is_set():
            ticks.append(time.monotonic())
            time.sleep(0.02)

    t = threading.Thread(target=tick)
    t.start()
    with pytest.raises(ufaigw.RequestTimeoutError):
        c.chat("m", MSGS)
    stop.set()
    t.join()
    assert len(ticks) > 10, len(ticks)
