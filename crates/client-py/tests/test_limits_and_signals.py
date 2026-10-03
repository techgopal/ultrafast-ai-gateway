"""Argument limits, and Ctrl-C reaching a call that is waiting on the network."""

import signal
import threading
import time

import pytest
import ultrafast
from conftest import KEY, OPENAI_CHAT, OPENAI_EMB, OPENAI_STREAM, Script, delta, sse

MSGS = [{"role": "user", "content": "hi"}]


def gw(url, **kw):
    return ultrafast.Client(ultrafast.gateway(url, KEY), **kw)


@pytest.mark.parametrize("bad", [-1, 2**32, 10**30])
def test_max_tokens_outside_u32_is_a_value_error(serve, bad):
    s = serve(Script.json(200, OPENAI_CHAT))
    with pytest.raises(ValueError):
        gw(s.url).chat("m", MSGS, max_tokens=bad)
    with pytest.raises(ValueError):
        gw(s.url).chat_stream("m", MSGS, max_tokens=bad)
    assert s.requests == []


@pytest.mark.parametrize("bad", [-1, 2**32])
def test_dimensions_outside_u32_is_a_value_error(serve, bad):
    s = serve(Script.json(200, OPENAI_EMB))
    with pytest.raises(ValueError):
        gw(s.url).embed("te3", "a", dimensions=bad)
    assert s.requests == []


@pytest.mark.parametrize("bad", [1.5, "8", True])
def test_max_tokens_must_be_an_int(serve, bad):
    s = serve(Script.json(200, OPENAI_CHAT))
    with pytest.raises(TypeError):
        gw(s.url).chat("m", MSGS, max_tokens=bad)


def test_u32_limits_are_accepted(serve):
    s = serve(Script.json(200, OPENAI_CHAT))
    gw(s.url).chat("m", MSGS, max_tokens=2**32 - 1)
    assert '"max_tokens":4294967295' in s.only()["body"].replace(" ", "")


def _interrupt_soon(delay=0.4):
    t = threading.Timer(delay, lambda: signal.raise_signal(signal.SIGINT))
    t.start()
    return t


def _interrupted(call):
    timer = _interrupt_soon()
    started = time.monotonic()
    try:
        with pytest.raises(KeyboardInterrupt):
            call()
    finally:
        timer.cancel()
    assert time.monotonic() - started < 5


def test_ctrl_c_interrupts_a_hanging_chat(serve):
    s = serve(Script.json(200, "{}", hang=True))
    _interrupted(lambda: gw(s.url, timeout=60).chat("m", MSGS))


def test_ctrl_c_interrupts_a_hanging_embed(serve):
    s = serve(Script.json(200, "{}", hang=True))
    _interrupted(lambda: gw(s.url, timeout=60).embed("te3", "a"))


def test_ctrl_c_interrupts_a_stream_that_stalls_and_it_stays_usable(serve):
    s = serve(Script.sse(sse(delta("a")), hang_after=True))
    stream = gw(s.url, timeout=60).chat_stream("m", MSGS)
    assert next(stream) == ultrafast.Delta("a")
    _interrupted(lambda: next(stream))
    stream.close()


def test_the_client_works_after_an_interrupt(serve):
    hung = serve(Script.json(200, "{}", hang=True))
    _interrupted(lambda: gw(hung.url, timeout=60).chat("m", MSGS))
    ok = serve(Script.json(200, OPENAI_CHAT))
    assert gw(ok.url).chat("m", MSGS).content == "hello"
