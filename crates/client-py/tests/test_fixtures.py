"""The shared parity fixtures (clients/fixtures/*.json): the Rust and TypeScript
clients run the same files and must agree with this one."""

import json
from pathlib import Path

import pytest
import ultrafast
from conftest import Script

FIXTURES = Path(__file__).resolve().parents[3] / "clients" / "fixtures"


def load(name):
    data = json.loads((FIXTURES / name).read_text(encoding="utf-8"))
    return data["key"], data["cases"]


KEY, REQUESTS = load("requests.json")
_, RESPONSES = load("responses.json")
_, ERRORS = load("errors.json")
_, STREAMS = load("streams.json")


def ids(cases):
    return [c["name"] for c in cases]


def target(t, base):
    url = t["base_url"].replace("{base}", base)
    kind = t["kind"]
    if kind == "gateway":
        return ultrafast.gateway(url, KEY)
    if kind == "openai_compatible":
        return ultrafast.openai_compatible(url, KEY)
    if kind == "azure":
        return ultrafast.azure(url, KEY, t.get("api_version"))
    return getattr(ultrafast, kind)(KEY, base_url=url)


def chat_kwargs(r):
    return {k: r[k] for k in ("max_tokens", "temperature", "top_p", "stop", "tags") if k in r}


def error_of(e):
    return {
        "kind": e.kind,
        "retryable": e.retryable,
        "status": e.status,
        "retry_after": e.retry_after,
        "message": e.message,
    }


def check_error(got, want):
    for k in ("kind", "retryable", "status", "retry_after"):
        assert got[k] == want[k], k
    if "message" in want:  # a fixture without a message leaves it out
        assert got["message"] == want["message"]


@pytest.mark.parametrize("case", REQUESTS, ids=ids(REQUESTS))
def test_request_on_the_wire(serve, case):
    op, r, want = case["op"], case["request"], case["expect"]
    script = Script.sse("data: [DONE]\n\n") if op == "chat_stream" else Script.json(200, "{}")
    s = serve(script)
    client = ultrafast.Client(target(case["target"], s.url))
    failure = None
    try:
        if op == "chat":
            client.chat(r["model"], r["messages"], **chat_kwargs(r))
        elif op == "embed":
            client.embed(r["model"], r["input"], **{k: r[k] for k in ("dimensions", "tags") if k in r})
        else:
            list(client.chat_stream(r["model"], r["messages"], **chat_kwargs(r)))
    except ultrafast.Error as e:
        failure = e
    if not want["sent"]:
        assert s.requests == [], "nothing may be sent"
        assert failure is not None
        check_error(error_of(failure), want["error"])
        return
    seen = s.only()
    # The mock only answers POST, so the method is checked by it being served.
    assert want["method"] == "POST"
    assert seen["path"] == want["path"]
    for k, v in want["headers"].items():
        assert seen["headers"].get(k) == v, k
    if "x-uf-tags" not in want["headers"]:
        assert "x-uf-tags" not in seen["headers"]
    assert KEY in seen["headers"][want["auth_header"]]
    assert json.loads(seen["body"]) == want["body"]


def usage(u):
    return None if u is None else {"input_tokens": u.input_tokens, "output_tokens": u.output_tokens}


def answer(serve, case):
    s = serve(Script(case["status"], [case["body"]], headers=case["headers"]))
    client = ultrafast.Client(target(case["target"], s.url))
    if case["op"] == "embed":
        r = client.embed(case.get("model", "m"), ["a"])
        return {"model": r.model, "vectors": r.vectors, "prompt_tokens": r.prompt_tokens}
    r = client.chat("m", [{"role": "user", "content": "hi"}])
    return {
        "id": r.id or "",
        "model": r.model or "",
        "content": r.content,
        "finish_reason": r.finish_reason,
        "usage": usage(r.usage),
    }


@pytest.mark.parametrize("case", RESPONSES, ids=ids(RESPONSES))
def test_parsed_response_or_error(serve, case):
    want = case["expect"]
    if "ok" in want:
        assert answer(serve, case) == want["ok"]
    else:
        with pytest.raises(ultrafast.Error) as ei:
            answer(serve, case)
        check_error(error_of(ei.value), want["error"])


@pytest.mark.parametrize("case", ERRORS, ids=ids(ERRORS))
def test_http_error(serve, case):
    with pytest.raises(ultrafast.Error) as ei:
        answer(serve, case)
    check_error(error_of(ei.value), case["expect"]["error"])


def event(e):
    if isinstance(e, ultrafast.Delta):
        return {"type": "delta", "text": e.text}
    return {"type": "done", "finish_reason": e.finish_reason, "usage": usage(e.usage)}


def run_stream(serve, case, chunks):
    s = serve(Script(200, chunks, content_type="text/event-stream"))
    client = ultrafast.Client(target(case["target"], s.url))
    events, error = [], None
    stream = client.chat_stream("m", [{"role": "user", "content": "hi"}])
    try:
        for e in stream:
            events.append(event(e))
    except ultrafast.Error as e:
        error = error_of(e)
        with pytest.raises(StopIteration):
            next(iter(stream))
    return events, error


@pytest.mark.parametrize("case", STREAMS, ids=ids(STREAMS))
@pytest.mark.parametrize("shape", ["pieces", "whole"])
def test_stream(serve, case, shape):
    chunks = case["chunks"] if shape == "pieces" else ["".join(case["chunks"])]
    events, error = run_stream(serve, case, chunks)
    assert events == case["expect"]["events"]
    want = case["expect"]["error"]
    if want is None:
        assert error is None
    else:
        assert error is not None
        check_error(error, want)
