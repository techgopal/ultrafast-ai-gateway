#!/usr/bin/env python3
"""A fake model provider that records every request it receives.

It answers the Anthropic Messages, OpenAI Responses and OpenAI Chat
Completions APIs with a short fixed text (streamed when the request asks for a
stream), and writes each request (method, path, query, headers, raw body) as a
JSON file into the recording directory. Used by scripts/record-agents.sh, to
refresh the fixtures under crates/gateway/tests/fixtures/agents/.

    agent-recorder.py <port> <recording-dir>

Port 0 picks a free port; the chosen port is printed as `listening <port>`.
"""
import json
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit

REPLY = "ok"
lock = threading.Lock()
counter = [0]


def sse(*events):
    out = b""
    for name, data in events:
        out += f"event: {name}\ndata: {json.dumps(data)}\n\n".encode()
    return out


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    rec_dir = Path(".")

    def log_message(self, *a):
        pass

    def _record(self, body):
        parts = urlsplit(self.path)
        with lock:
            counter[0] += 1
            n = counter[0]
        name = parts.path.strip("/").replace("/", "_") or "root"
        entry = {
            "method": self.command,
            "path": parts.path,
            "query": parts.query,
            "headers": [[k, v] for k, v in self.headers.items()],
            "body": body.decode("utf-8", "replace"),
            "at": time.time(),
        }
        (self.rec_dir / f"{n:03d}-{self.command}-{name}.json").write_text(
            json.dumps(entry, indent=1)
        )

    def _send(self, status, ctype, payload, extra=()):
        self.send_response(status)
        self.send_header("content-type", ctype)
        self.send_header("content-length", str(len(payload)))
        for k, v in extra:
            self.send_header(k, v)
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(payload)

    def _json(self, status, obj):
        self._send(status, "application/json", json.dumps(obj).encode())

    def do_HEAD(self):
        self._record(b"")
        self._send(200, "text/plain", b"")

    def do_GET(self):
        self._record(b"")
        if urlsplit(self.path).path.endswith("/models"):
            self._json(200, {"data": [], "has_more": False})
        else:
            self._json(200, {})

    def do_POST(self):
        n = int(self.headers.get("content-length") or 0)
        body = self.rfile.read(n) if n else b""
        self._record(body)
        path = urlsplit(self.path).path
        try:
            req = json.loads(body or b"{}")
        except ValueError:
            req = {}
        stream = bool(req.get("stream"))
        model = req.get("model", "m")
        if path.endswith("/count_tokens"):
            return self._json(200, {"input_tokens": 1})
        if path.endswith("/messages"):
            return self._messages(model, stream)
        if path.endswith("/responses"):
            return self._responses(model, stream)
        if path.endswith("/chat/completions"):
            return self._chat(model, stream)
        self._json(404, {"error": {"message": "not recorded"}})

    def _messages(self, model, stream):
        usage = {"input_tokens": 5, "output_tokens": 2}
        if not stream:
            return self._json(
                200,
                {
                    "id": "msg_rec", "type": "message", "role": "assistant", "model": model,
                    "content": [{"type": "text", "text": REPLY}],
                    "stop_reason": "end_turn", "stop_sequence": None, "usage": usage,
                },
            )
        msg = {"id": "msg_rec", "type": "message", "role": "assistant", "model": model,
               "content": [], "stop_reason": None, "usage": {"input_tokens": 5, "output_tokens": 1}}
        payload = sse(
            ("message_start", {"type": "message_start", "message": msg}),
            ("content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
            ("ping", {"type": "ping"}),
            ("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": REPLY}}),
            ("content_block_stop", {"type": "content_block_stop", "index": 0}),
            ("message_delta", {"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 2}}),
            ("message_stop", {"type": "message_stop"}),
        )
        self._send(200, "text/event-stream", payload)

    def _responses(self, model, stream):
        usage = {"input_tokens": 5, "output_tokens": 2, "total_tokens": 7,
                 "input_tokens_details": {"cached_tokens": 0}, "output_tokens_details": {"reasoning_tokens": 0}}
        item = {"type": "message", "id": "msg_rec", "role": "assistant", "status": "completed",
                "content": [{"type": "output_text", "text": REPLY, "annotations": []}]}
        done = {"id": "resp_rec", "object": "response", "status": "completed", "model": model,
                "output": [item], "usage": usage}
        if not stream:
            return self._json(200, done)
        self._send(
            200, "text/event-stream",
            sse(
                ("response.created", {"type": "response.created", "sequence_number": 0, "response": dict(done, status="in_progress", output=[], usage=None)}),
                ("response.output_item.added", {"type": "response.output_item.added", "sequence_number": 1, "output_index": 0, "item": dict(item, content=[], status="in_progress")}),
                ("response.output_text.delta", {"type": "response.output_text.delta", "sequence_number": 2, "output_index": 0, "content_index": 0, "item_id": "msg_rec", "delta": REPLY}),
                ("response.output_item.done", {"type": "response.output_item.done", "sequence_number": 3, "output_index": 0, "item": item}),
                ("response.completed", {"type": "response.completed", "sequence_number": 4, "response": done}),
            ),
        )

    def _chat(self, model, stream):
        usage = {"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7}
        if not stream:
            return self._json(
                200,
                {"id": "chatcmpl-rec", "object": "chat.completion", "created": 1, "model": model,
                 "choices": [{"index": 0, "message": {"role": "assistant", "content": REPLY}, "finish_reason": "stop"}],
                 "usage": usage},
            )
        def chunk(delta, finish=None, u=None):
            c = {"id": "chatcmpl-rec", "object": "chat.completion.chunk", "created": 1, "model": model,
                 "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
            if u:
                c["usage"] = u
            return "data: " + json.dumps(c) + "\n\n"
        text = chunk({"role": "assistant", "content": REPLY}) + chunk({}, "stop", usage) + "data: [DONE]\n\n"
        self._send(200, "text/event-stream", text.encode())


def main():
    port = int(sys.argv[1])
    rec = Path(sys.argv[2])
    rec.mkdir(parents=True, exist_ok=True)
    Handler.rec_dir = rec
    srv = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"listening {srv.server_address[1]}", flush=True)
    srv.serve_forever()


if __name__ == "__main__":
    main()
