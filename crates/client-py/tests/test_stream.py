import ultrafast
from conftest import KEY, OPENAI_STREAM, Script, delta, sse

EXPECTED = [
    ultrafast.Delta("hé"),
    ultrafast.Delta("llo"),
    ultrafast.Done("stop", ultrafast.Usage(3, 2)),
]
MSGS = [{"role": "user", "content": "hi"}]


def test_stream_yields_events_in_order(serve):
    s = serve(Script.sse(OPENAI_STREAM))
    c = ultrafast.Client(ultrafast.gateway(s.url, KEY))
    assert list(c.chat_stream("m", MSGS)) == EXPECTED
    req = s.only()
    assert '"stream":true' in req["body"].replace(" ", "")
    assert req["headers"]["accept"] == "text/event-stream"


def test_stream_split_at_every_few_bytes(serve):
    data = OPENAI_STREAM.encode()
    for i in range(1, len(data), 37):
        s = serve(Script(200, [data[:i], data[i:]], content_type="text/event-stream"))
        c = ultrafast.Client(ultrafast.gateway(s.url, KEY))
        assert list(c.chat_stream("m", MSGS)) == EXPECTED, i


def test_text_then_an_error_event_gives_the_text_then_a_typed_error(serve):
    body = sse(delta("par"), delta("tial"), '{"error":{"message":"overloaded","type":"overloaded_error"}}')
    s = serve(Script.sse(body))
    it = iter(ultrafast.Client(ultrafast.gateway(s.url, KEY)).chat_stream("m", MSGS))
    assert next(it) == ultrafast.Delta("par")
    assert next(it) == ultrafast.Delta("tial")
    try:
        next(it)
    except ultrafast.Error as e:
        assert e.kind in ("upstream", "malformed", "rate_limited", "invalid_request"), e.kind
    else:
        raise AssertionError("no error after the text")
    try:
        next(it)
    except StopIteration:
        pass
    else:
        raise AssertionError("an error ends the stream")


def test_a_stream_cut_short_is_an_error_not_a_silent_end(serve):
    s = serve(Script.sse(sse(delta("par"))))  # no [DONE], connection closes
    it = iter(ultrafast.Client(ultrafast.gateway(s.url, KEY)).chat_stream("m", MSGS))
    assert next(it) == ultrafast.Delta("par")
    try:
        next(it)
    except ultrafast.MalformedError as e:
        assert not e.retryable
    else:
        raise AssertionError("silent truncation")


def test_an_error_status_raises_at_the_call(serve):
    import pytest

    s = serve(Script.json(429, '{"error":{"message":"slow down","type":"x"}}', headers={"Retry-After": "7"}))
    c = ultrafast.Client(ultrafast.gateway(s.url, KEY))
    with pytest.raises(ultrafast.RateLimitError) as ei:
        c.chat_stream("m", MSGS)
    assert ei.value.retry_after == 7


def test_stream_can_be_closed_early_and_is_a_context_manager(serve):
    s = serve(Script.sse(OPENAI_STREAM))
    c = ultrafast.Client(ultrafast.gateway(s.url, KEY))
    with c.chat_stream("m", MSGS) as stream:
        assert next(stream) == EXPECTED[0]
    assert list(stream) == []
