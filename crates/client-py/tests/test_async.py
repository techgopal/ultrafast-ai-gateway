import asyncio

import pytest
import ultrafast
from conftest import KEY, OPENAI_CHAT, OPENAI_EMB, OPENAI_STREAM, Script, delta, sse

pytestmark = pytest.mark.asyncio
MSGS = [{"role": "user", "content": "hi"}]
EXPECTED = [
    ultrafast.Delta("hé"),
    ultrafast.Delta("llo"),
    ultrafast.Done("stop", ultrafast.Usage(3, 2)),
]


def gw(url, **kw):
    return ultrafast.AsyncClient(ultrafast.gateway(url, KEY), **kw)


async def test_chat(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    r = await gw(s.url).chat("m", MSGS, tags={"team": "a"})
    assert r.content == "hello" and r.usage == ultrafast.Usage(3, 2)
    assert s.only()["headers"]["x-uf-tags"] == '{"team":"a"}'


async def test_embed(serve):
    s = serve(Script.json(200, OPENAI_EMB))
    r = await gw(s.url).embed("te3", ["a", "b"])
    assert r.vectors == [[0.5], [0.25]]


async def test_stream_with_async_for(serve):
    s = serve(Script.sse(OPENAI_STREAM))
    got = [e async for e in gw(s.url).chat_stream("m", MSGS)]
    assert got == EXPECTED


async def test_stream_can_be_awaited_first(serve):
    s = serve(Script.sse(OPENAI_STREAM))
    stream = await gw(s.url).chat_stream("m", MSGS)
    assert [e async for e in stream] == EXPECTED


async def test_text_then_error(serve):
    s = serve(Script.sse(sse(delta("par"))))
    got = []
    with pytest.raises(ultrafast.MalformedError):
        async for e in gw(s.url).chat_stream("m", MSGS):
            got.append(e)
    assert got == [ultrafast.Delta("par")]


async def test_errors_and_retry_after(serve):
    s = serve(Script.json(429, '{"error":{"message":"slow","type":"x"}}', headers={"Retry-After": "5"}))
    with pytest.raises(ultrafast.RateLimitError) as ei:
        await gw(s.url).chat("m", MSGS)
    assert ei.value.retry_after == 5 and ei.value.retryable
    with pytest.raises(ultrafast.RateLimitError):
        async for _ in gw(s.url).chat_stream("m", MSGS):
            pass


async def test_connection_failure_has_no_key(dead_url):
    with pytest.raises(ultrafast.NetworkError) as ei:
        await gw(dead_url).chat("m", MSGS)
    assert KEY not in f"{ei.value} {ei.value!r}"


async def test_timeout(serve):
    s = serve(Script.json(200, "{}"))
    s.script.hang = True
    with pytest.raises(ultrafast.RequestTimeoutError):
        await gw(s.url, timeout=0.3).chat("m", MSGS)


async def test_calls_run_concurrently(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    c = gw(s.url)
    rs = await asyncio.gather(*[c.chat("m", MSGS) for _ in range(8)])
    assert [r.content for r in rs] == ["hello"] * 8
    assert len(s.requests) == 8


async def test_cancellation_does_not_break_the_client(serve):
    slow = serve(Script.json(200, "{}"))
    slow.script.hang = True
    c = gw(slow.url)
    task = asyncio.ensure_future(c.chat("m", MSGS))
    await asyncio.sleep(0.1)
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await task
    ok = serve(Script.json(200, OPENAI_CHAT))
    assert (await gw(ok.url).chat("m", MSGS)).content == "hello"


async def test_repr_has_no_key():
    c = ultrafast.AsyncClient(ultrafast.openai(KEY))
    assert KEY not in repr(c)
