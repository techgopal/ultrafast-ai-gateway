import pytest
import ultrafast
from conftest import KEY, OPENAI_CHAT, Script

MSGS = [{"role": "user", "content": "hi"}]


def gw(url):
    return ultrafast.Client(ultrafast.gateway(url, KEY))


@pytest.mark.parametrize(
    "status,cls,kind,retryable",
    [
        (400, ultrafast.InvalidRequestError, "invalid_request", False),
        (401, ultrafast.AuthenticationError, "auth", False),
        (403, ultrafast.PermissionDeniedError, "permission", False),
        (404, ultrafast.NotFoundError, "not_found", False),
        (408, ultrafast.RequestTimeoutError, "timeout", True),
        (429, ultrafast.RateLimitError, "rate_limited", True),
        (500, ultrafast.UpstreamError, "upstream", True),
        (503, ultrafast.UpstreamError, "upstream", True),
    ],
)
def test_statuses_map_to_typed_errors(serve, status, cls, kind, retryable):
    s = serve(Script.json(status, '{"error":{"message":"nope","type":"x"}}'))
    with pytest.raises(cls) as ei:
        gw(s.url).chat("m", MSGS)
    e = ei.value
    assert isinstance(e, ultrafast.Error)
    assert (e.kind, e.retryable, e.status) == (kind, retryable, status)
    assert "nope" in str(e)


def test_error_classes_do_not_shadow_builtins():
    assert ultrafast.RequestTimeoutError is not TimeoutError
    assert ultrafast.PermissionDeniedError is not PermissionError
    assert issubclass(ultrafast.Error, Exception)
    for name in ("AuthenticationError PermissionDeniedError NotFoundError InvalidRequestError "
                 "RateLimitError UpstreamError NetworkError RequestTimeoutError MalformedError").split():
        assert issubclass(getattr(ultrafast, name), ultrafast.Error)


@pytest.mark.parametrize("target", ["gateway", "openai", "anthropic"])
def test_429_with_retry_after_is_rate_limited_on_every_target(serve, target):
    s = serve(Script.json(429, '{"error":{"message":"slow","type":"x"}}', headers={"Retry-After": "12"}))
    if target == "gateway":
        t = ultrafast.gateway(s.url, KEY)
    elif target == "openai":
        t = ultrafast.openai(KEY, base_url=s.url + "/v1")
    else:
        t = ultrafast.anthropic(KEY, base_url=s.url)
    with pytest.raises(ultrafast.RateLimitError) as ei:
        ultrafast.Client(t).chat("m", MSGS)
    e = ei.value
    assert e.kind == "rate_limited" and e.retryable and e.retry_after == 12 and e.status == 429


def test_429_without_retry_after_has_none(serve):
    s = serve(Script.json(429, '{"error":{"message":"slow","type":"x"}}'))
    with pytest.raises(ultrafast.RateLimitError) as ei:
        gw(s.url).chat("m", MSGS)
    assert ei.value.retry_after is None


def test_refused_connection_is_a_retryable_network_error_without_the_key(dead_url):
    with pytest.raises(ultrafast.NetworkError) as ei:
        gw(dead_url).chat("m", MSGS)
    e = ei.value
    assert e.retryable and e.status is None
    assert KEY not in f"{e} {e!r} {e.args} {vars(e)}"


def test_a_server_that_never_answers_times_out(serve):
    s = serve(Script.json(200, "{}"))
    s.script.hang = True
    c = ultrafast.Client(ultrafast.gateway(s.url, KEY), timeout=0.3)
    with pytest.raises(ultrafast.RequestTimeoutError) as ei:
        c.chat("m", MSGS)
    assert ei.value.retryable


def test_unreadable_success_is_malformed(serve):
    s = serve(Script.json(200, "not json"))
    with pytest.raises(ultrafast.MalformedError) as ei:
        gw(s.url).chat("m", MSGS)
    assert not ei.value.retryable


def test_a_body_over_the_cap_is_malformed(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    c = ultrafast.Client(ultrafast.gateway(s.url, KEY), max_response_bytes=10)
    with pytest.raises(ultrafast.MalformedError):
        c.chat("m", MSGS)


def test_a_redirect_is_refused_and_not_followed(serve):
    target = serve(Script.json(200, OPENAI_CHAT))
    s = serve(Script(302, [], headers={"Location": target.url + "/v1/chat/completions"}))
    with pytest.raises(ultrafast.InvalidRequestError):
        gw(s.url).chat("m", MSGS)
    assert target.requests == []


def test_the_key_is_in_no_error_and_no_repr(serve, dead_url):
    echo = f'{{"error":{{"message":"bad key {KEY} here","type":"x"}}}}'
    shown = []
    for status in (400, 401, 403, 404, 429, 500):
        s = serve(Script.json(status, echo))
        with pytest.raises(ultrafast.Error) as ei:
            gw(s.url).chat("m", MSGS)
        e = ei.value
        shown.append(f"{e} {e!r} {e.args} {e.__dict__}")
    s = serve(Script.json(200, OPENAI_CHAT))
    c = ultrafast.Client(ultrafast.gateway(s.url, KEY))
    t = ultrafast.openai(KEY)
    shown += [repr(c), str(c), repr(t), str(t), repr(ultrafast.AsyncClient(t))]
    shown += [repr(ultrafast.anthropic(KEY)), repr(ultrafast.azure("https://x", KEY, "v"))]
    shown += [repr(c.chat("m", MSGS))]
    for text in shown:
        assert KEY not in text, text
    # The short key too.
    s = serve(Script.json(401, '{"error":{"message":"key k1 is wrong","type":"x"}}'))
    with pytest.raises(ultrafast.AuthenticationError) as ei:
        ultrafast.Client(ultrafast.gateway(s.url, "k1")).chat("m", MSGS)
    assert "k1" not in str(ei.value)


def test_a_stream_error_message_is_scrubbed(serve):
    from conftest import delta, sse

    body = sse(delta("x"), f'{{"error":{{"message":"bad {KEY}","type":"x"}}}}')
    s = serve(Script.sse(body))
    it = iter(ultrafast.Client(ultrafast.gateway(s.url, KEY)).chat_stream("m", MSGS))
    next(it)
    with pytest.raises(ultrafast.Error) as ei:
        next(it)
    assert KEY not in f"{ei.value} {ei.value!r}"


def test_nothing_is_retried(serve):
    s = serve(Script.json(500, '{"error":{"message":"x","type":"x"}}'))
    with pytest.raises(ultrafast.UpstreamError):
        gw(s.url).chat("m", MSGS)
    assert len(s.requests) == 1
