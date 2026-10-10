"""A local HTTP server that misbehaves on purpose: it answers slowly, stalls after
the headers, or drips its body, so the timeout paths run for real (no mock)."""

from __future__ import annotations

import socket
import threading
import time
from collections.abc import Iterator
from contextlib import contextmanager

STALL_FOR_S = 5.0


@contextmanager
def slow_server(mode: str, status: int = 200) -> Iterator[str]:
    """`mode`: "silent" (no answer), "stall" (headers, then nothing), "drip" (a byte every 0.1 s), "headers_drip" (a byte of the headers every 0.1 s)."""
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(16)
    listener.settimeout(0.2)
    stop = threading.Event()

    def serve(connection: socket.socket) -> None:
        with connection:
            connection.settimeout(1.0)
            try:
                connection.recv(65536)
                if mode == "silent":
                    stop.wait(STALL_FOR_S)
                    return
                if mode == "headers_drip":
                    connection.sendall(b"HTTP/1.1 200 X\r\nx-pad: ")
                    end = time.monotonic() + STALL_FOR_S
                    while time.monotonic() < end and not stop.is_set():
                        connection.sendall(b"a")
                        time.sleep(0.1)
                    return
                connection.sendall(
                    f"HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: 100000\r\n\r\n{{".encode()
                )
                if mode == "stall":
                    stop.wait(STALL_FOR_S)
                    return
                end = time.monotonic() + STALL_FOR_S
                while time.monotonic() < end and not stop.is_set():
                    connection.sendall(b" ")
                    time.sleep(0.1)
            except OSError:
                return

    def accept() -> None:
        while not stop.is_set():
            try:
                connection, _ = listener.accept()
            except (TimeoutError, OSError):
                continue
            threading.Thread(target=serve, args=(connection,), daemon=True).start()

    thread = threading.Thread(target=accept, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{listener.getsockname()[1]}"
    finally:
        stop.set()
        thread.join(2)
        listener.close()
