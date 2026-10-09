"""Starts the real gateway (a debug build) for the SDK tests.

It runs on a port the system gives out (never 3900), with a data directory of
its own and an environment written out in full, so no `UF_*` variable of the
shell reaches it. It is stopped by its process id and its directory removed when
the fixture ends and when the test process exits.

The binary is `UF_E2E_BINARY` when set, else `target/debug/ultrafast` of this
repository, built with `cargo build -p ultrafast-gateway` (see `conftest.py`).
"""

from __future__ import annotations

import atexit
import os
import secrets
import shutil
import socket
import subprocess
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

import httpx

IN_USE_PORT = 3900
BINARY_VARIABLE = "UF_E2E_BINARY"
REPOSITORY = Path(__file__).resolve().parents[3]
DEPLOYED = Path.home() / ".local" / "share" / "ultrafast-gateway"
READY_WITHIN_S = 30.0
STOP_WITHIN_S = 10.0


@dataclass
class Account:
    email: str
    password: str


@dataclass
class Gateway:
    origin: str
    port: int
    pid: int
    admin: Account
    process: subprocess.Popen[bytes] = field(repr=False)
    data_dir: str = field(repr=False)
    lines: list[str] = field(default_factory=list, repr=False)

    def stop(self) -> None:
        _stop(self.process, self.data_dir)


def _is_deployed(path: Path) -> bool:
    candidates = {path, path.resolve()}
    deployed = {DEPLOYED, DEPLOYED.resolve()}
    return any(c == d or d in c.parents for c in candidates for d in deployed)


def binary_path() -> Path:
    given = os.environ.get(BINARY_VARIABLE, "")
    path = Path(given).resolve() if given else REPOSITORY / "target" / "debug" / "ultrafast"
    if _is_deployed(path):
        raise RuntimeError(f"{BINARY_VARIABLE} names a binary of the deployed gateway; it is not started.")
    if not os.access(path, os.X_OK):
        raise RuntimeError(f"No gateway binary at {path}. Build it: cargo build -p ultrafast-gateway")
    return path


def free_port() -> int:
    while True:
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = int(sock.getsockname()[1])
        if port != IN_USE_PORT:
            return port


_running: dict[int, tuple[subprocess.Popen[bytes], str]] = {}


@atexit.register
def _cleanup() -> None:
    for process, data_dir in list(_running.values()):
        _stop(process, data_dir)


def _stop(process: subprocess.Popen[bytes], data_dir: str) -> None:
    # Only a process this module started, stopped by its own handle.
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(STOP_WITHIN_S)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    _running.pop(process.pid, None)
    shutil.rmtree(data_dir, ignore_errors=True)


def _healthy(origin: str) -> bool:
    try:
        return httpx.get(f"{origin}/health", timeout=2.0).is_success
    except httpx.HTTPError:
        return False


def start_gateway() -> Gateway:
    """Starts a gateway with a first admin. Retries when another process took the port."""
    binary = binary_path()
    admin = Account("admin@example.com", f"pw-{secrets.token_hex(12)}")
    for _ in range(3):
        data_dir = tempfile.mkdtemp(prefix="uf-e2e-")
        port = free_port()
        origin = f"http://127.0.0.1:{port}"
        env = {
            "UF_DATA_DIR": data_dir,
            "UF_HOST": "127.0.0.1",
            "UF_PORT": str(port),
            "UF_INSECURE_COOKIES": "true",
            "UF_ADMIN_EMAIL": admin.email,
            "UF_ADMIN_PASSWORD": admin.password,
            "RUST_LOG": "warn",
            "NO_COLOR": "1",
        }
        log = Path(data_dir + ".log")
        with log.open("wb") as sink:
            process = subprocess.Popen(
                [str(binary), "serve"], env=env, stdin=subprocess.DEVNULL, stdout=sink, stderr=sink
            )
        _running[process.pid] = (process, data_dir)
        until = time.monotonic() + READY_WITHIN_S
        while process.poll() is None and time.monotonic() < until:
            if _healthy(origin):
                log.unlink(missing_ok=True)
                return Gateway(origin, port, process.pid, admin, process, data_dir)
            time.sleep(0.1)
        output = log.read_text(errors="replace")
        _stop(process, data_dir)
        log.unlink(missing_ok=True)
        if "could not listen" not in output:
            raise RuntimeError(f"The gateway did not start. Its output:\n{output}")
    raise RuntimeError("The gateway did not start: no free port.")
