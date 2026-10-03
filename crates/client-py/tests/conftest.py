"""A mock HTTP server on a free port; tests script its answers and read what it saw."""

import json
import socket
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

KEY = "sk-test-SECRET-0123456789"

OPENAI_CHAT = json.dumps(
    {
        "id": "c1",
        "model": "gpt-4o",
        "choices": [
            {"message": {"role": "assistant", "content": "hello"}, "finish_reason": "stop"}
        ],
        "usage": {"prompt_tokens": 3, "completion_tokens": 2},
    }
)
ANTHROPIC_CHAT = json.dumps(
    {
        "id": "m1",
        "model": "claude-sonnet-5",
        "content": [{"type": "text", "text": "hello"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 3, "output_tokens": 2},
    }
)
OPENAI_EMB = json.dumps(
    {
        "model": "te3",
        "data": [
            {"index": 1, "embedding": [0.25]},
            {"index": 0, "embedding": [0.5]},
        ],
        "usage": {"prompt_tokens": 4},
    }
)


def sse(*events):
    return "".join(f"data: {e}\n\n" for e in events)


def delta(text):
    return json.dumps({"choices": [{"delta": {"content": text}, "finish_reason": None}]})


OPENAI_STREAM = sse(
    json.dumps({"choices": [{"delta": {"role": "assistant"}, "finish_reason": None}]}),
    delta("hé"),
    delta("llo"),
    json.dumps({"choices": [{"delta": {}, "finish_reason": "stop"}]}),
    json.dumps({"choices": [], "usage": {"prompt_tokens": 3, "completion_tokens": 2}}),
    "[DONE]",
)


class Script:
    """What the server answers: a status, headers, then body chunks (bytes)."""

    def __init__(self, status=200, chunks=(), headers=None, hang=False, content_type=None, hang_after=False):
        self.status = status
        self.chunks = [c.encode() if isinstance(c, str) else c for c in chunks]
        self.headers = dict(headers or {})
        self.hang = hang
        self.hang_after = hang_after  # send the chunks, then stall instead of ending
        self.content_type = content_type

    @classmethod
    def json(cls, status, body, **kw):
        return cls(status, [body], content_type="application/json", **kw)

    @classmethod
    def sse(cls, body, **kw):
        return cls(200, [body], content_type="text/event-stream", **kw)


class Server:
    def __init__(self, script):
        self.script = script
        self.requests = []
        self.stop = threading.Event()
        server = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *a):
                pass

            def do_POST(self):
                n = int(self.headers.get("content-length") or 0)
                body = self.rfile.read(n)
                server.requests.append(
                    {
                        "path": self.path,
                        "headers": {k.lower(): v for k, v in self.headers.items()},
                        "body": body.decode(),
                    }
                )
                s = server.script
                if s.hang:
                    server.stop.wait(30)
                    return
                self.send_response(s.status)
                if s.content_type:
                    self.send_header("content-type", s.content_type)
                for k, v in s.headers.items():
                    self.send_header(k, v)
                self.end_headers()
                for chunk in s.chunks:
                    self.wfile.write(chunk)
                    self.wfile.flush()
                    time.sleep(0.01)
                if s.hang_after:
                    server.stop.wait(30)
                # HTTP/1.0: closing the connection ends the body.

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.httpd.daemon_threads = True
        self.url = f"http://127.0.0.1:{self.httpd.server_address[1]}"
        self.thread = threading.Thread(target=self.httpd.serve_forever, kwargs={"poll_interval": 0.02}, daemon=True)
        self.thread.start()

    def only(self):
        assert len(self.requests) == 1, self.requests
        return self.requests[0]

    def close(self):
        self.stop.set()
        self.httpd.shutdown()
        self.httpd.server_close()


@pytest.fixture
def serve():
    servers = []

    def make(script):
        s = Server(script)
        servers.append(s)
        return s

    yield make
    for s in servers:
        s.close()


@pytest.fixture
def dead_url():
    """A URL nothing listens on."""
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return f"http://127.0.0.1:{port}"
