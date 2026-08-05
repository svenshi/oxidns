#!/usr/bin/env python3
"""Credential-free, loopback-only backend-decoupling acceptance harness."""

from __future__ import annotations

import argparse
import hashlib
import json
import signal
import socket
import struct
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def reserve_port(kind: int) -> int:
    with socket.socket(socket.AF_INET, kind) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def request(base: str, method: str, path: str, body: dict | None = None) -> tuple[int, dict]:
    data = None if body is None else json.dumps(body, separators=(",", ":")).encode()
    request = urllib.request.Request(
        base + path,
        data=data,
        method=method,
        headers={"Accept": "application/json", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            raw = response.read()
            return int(response.status), decode_body(raw)
    except urllib.error.HTTPError as error:
        raw = error.read()
        return int(error.code), decode_body(raw)


def decode_body(raw: bytes) -> dict:
    if not raw:
        return {}
    try:
        value = json.loads(raw)
        return value if isinstance(value, dict) else {"value": value}
    except json.JSONDecodeError:
        return {"raw": raw.decode("utf-8", errors="replace")}


def wait_api(base: str) -> None:
    last: object = None
    for _ in range(120):
        try:
            status, body = request(base, "GET", "/build")
            if status == 200 and body.get("ok"):
                return
            last = body
        except Exception as error:  # noqa: BLE001 - acceptance diagnostics
            last = error
        time.sleep(0.1)
    raise RuntimeError(f"API did not become ready: {last}")


def wait_transaction(base: str, transaction_id: str, expected: str) -> dict:
    last: dict = {}
    for _ in range(160):
        try:
            status, body = request(base, "GET", "/config/apply/status")
            if status == 200 and body.get("transaction"):
                last = body["transaction"]
                if last.get("transaction_id") == transaction_id and last.get("status") == expected:
                    return last
        except Exception:  # API is briefly unavailable while runtime swaps
            pass
        time.sleep(0.1)
    raise RuntimeError(f"transaction did not reach {expected}: {last}")


def question_end(packet: bytes) -> int:
    offset = 12
    while offset < len(packet) and packet[offset] != 0:
        offset += 1 + packet[offset]
    require(offset + 5 <= len(packet), "truncated DNS question")
    return offset + 5


def dns_response(packet: bytes, answer: str) -> bytes:
    end = question_end(packet)
    header = struct.pack("!HHHHHH", struct.unpack("!H", packet[:2])[0], 0x8180, 1, 1, 0, 0)
    record = b"\xc0\x0c" + struct.pack("!HHIH", 1, 1, 60, 4) + socket.inet_aton(answer)
    return header + packet[12:end] + record


class MockDns:
    def __init__(self, answer: str):
        self.answer = answer
        self.requests = 0
        self.stop = threading.Event()
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.bind(("127.0.0.1", 0))
        self.socket.settimeout(0.2)
        self.port = int(self.socket.getsockname()[1])
        self.thread = threading.Thread(target=self.run, daemon=True)

    def run(self) -> None:
        while not self.stop.is_set():
            try:
                packet, peer = self.socket.recvfrom(4096)
                self.socket.sendto(dns_response(packet, self.answer), peer)
                self.requests += 1
            except socket.timeout:
                continue
            except OSError:
                return

    def close(self) -> None:
        self.stop.set()
        self.socket.close()
        self.thread.join(timeout=2)


def dns_query(port: int, expected: str) -> None:
    labels = b"".join(bytes([len(label)]) + label.encode() for label in "acceptance.test".split("."))
    packet = struct.pack("!HHHHHH", 0x4F58, 0x0100, 1, 0, 0, 0) + labels + b"\0" + struct.pack("!HH", 1, 1)
    last: object = None
    for _ in range(60):
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
                sock.settimeout(0.5)
                sock.sendto(packet, ("127.0.0.1", port))
                response, _ = sock.recvfrom(4096)
            require(socket.inet_aton(expected) in response, "unexpected DNS answer")
            return
        except Exception as error:  # noqa: BLE001 - bounded retry diagnostics
            last = error
            time.sleep(0.1)
    raise RuntimeError(f"DNS query failed: {last}")


def candidate_yaml(api_port: int, dns_port: int, upstream_port: int, fail_assembly: bool = False) -> str:
    failure_provider = ""
    failure_rule = ""
    if fail_assembly:
        failure_provider = """
  - tag: missing_domain_data
    type: domain_set
    args:
      files:
        - definitely-missing-oxidns-acceptance-rules.txt
"""
        failure_rule = """      - matches: qname $missing_domain_data
        exec: accept
"""
    return f"""log:
  level: warn
api:
  http:
    listen: 127.0.0.1:{api_port}
plugins:
{failure_provider}  - tag: native_forward
    type: forward
    args:
      upstreams:
        - tag: loopback
          addr: udp://127.0.0.1:{upstream_port}
  - tag: native_path
    type: sequence
    args:
{failure_rule}      - exec: $native_forward
      - exec: accept
  - tag: native_udp
    type: udp_server
    args:
      listen: 127.0.0.1:{dns_port}
      entry: native_path
"""


def stop_process(process: subprocess.Popen[bytes] | None) -> None:
    if process is None or process.poll() is not None:
        return
    process.send_signal(signal.SIGTERM)
    try:
        process.wait(timeout=8)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--expected-sha256", required=True)
    args = parser.parse_args()
    binary = Path(args.binary).resolve()
    require(sha256(binary) == args.expected_sha256, "binary hash mismatch")

    api_port = reserve_port(socket.SOCK_STREAM)
    dns_port = reserve_port(socket.SOCK_DGRAM)
    mock = MockDns("192.0.2.44")
    mock.thread.start()
    process: subprocess.Popen[bytes] | None = None
    checks: list[str] = []

    with tempfile.TemporaryDirectory(prefix="oxidns-decoupling-") as directory:
        run = Path(directory)
        config = run / "config.yaml"
        initial = f"log:\n  level: warn\napi:\n  http:\n    listen: 127.0.0.1:{api_port}\nplugins: []\n"
        config.write_text(initial, encoding="utf-8")
        log = (run / "oxidns.log").open("wb")
        try:
            process = subprocess.Popen(
                [str(binary), "start", "-c", str(config), "-d", str(run), "-l", "warn"],
                stdout=log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            base = f"http://127.0.0.1:{api_port}/api"
            wait_api(base)
            status, build = request(base, "GET", "/build")
            require(status == 200 and build["build"]["bundle"] == "standard", "not a Standard bundle")
            checks.append("standard Cargo bundle remains available")

            status, _ = request(base, "GET", "/standard/plan")
            require(status == 404, "removed /api/standard route is still registered")
            status, workspace = request(base, "GET", "/webui/config")
            require(status == 200 and workspace["config"] == {"schema": 1}, "WebUI default is not opaque")
            checks.append("Standard APIs return 404 and WebUI JSON is opaque")

            status, current = request(base, "GET", "/config")
            require(status == 200, "cannot read initial config")
            candidate = candidate_yaml(api_port, dns_port, mock.port)
            status, validated = request(base, "POST", "/config/validate", {"format": "yaml", "content": candidate})
            require(status == 200 and validated.get("version"), f"candidate validation failed: {validated}")
            checks.append("real-directory native YAML validation")

            before_forgery = config.read_bytes()
            status, _ = request(base, "POST", "/config/apply", {
                "format": "yaml", "content": candidate, "base_version": current["version"], "candidate_version": "forged"
            })
            require(status == 409 and config.read_bytes() == before_forgery, "forged candidate changed disk")

            status, accepted = request(base, "POST", "/config/apply", {
                "format": "yaml", "content": candidate, "base_version": current["version"],
                "candidate_version": validated["version"],
            })
            require(status == 202 and accepted["transaction_id"].startswith("config-"), f"apply rejected: {accepted}")
            wait_transaction(base, accepted["transaction_id"], "succeeded")
            dns_query(dns_port, "192.0.2.44")
            require(mock.requests > 0, "runtime did not query the loopback upstream")
            checks.append("atomic Apply, runtime reload, and real DNS query")

            status, history = request(base, "GET", "/config/history")
            require(status == 200 and history["entries"], "healthy history is empty")
            disk_before_preview = config.read_bytes()
            status, preview = request(base, "POST", "/config/history/restore", {"id": history["entries"][0]["id"]})
            require(status == 200 and preview["content"] == candidate, "history preview mismatch")
            require(config.read_bytes() == disk_before_preview, "history preview wrote configuration")
            checks.append("bounded healthy history and read-only restore preview")

            failing = candidate_yaml(api_port, dns_port, mock.port, fail_assembly=True)
            _, current = request(base, "GET", "/config")
            status, validated = request(base, "POST", "/config/validate", {"format": "yaml", "content": failing})
            require(status == 200, f"assembly-failure candidate did not validate: {validated}")
            status, accepted = request(base, "POST", "/config/apply", {
                "format": "yaml", "content": failing, "base_version": current["version"],
                "candidate_version": validated["version"],
            })
            require(status == 202, f"assembly-failure candidate was not accepted: {accepted}")
            wait_api(base)
            wait_transaction(base, accepted["transaction_id"], "failed")
            require(config.read_text(encoding="utf-8") == candidate, "failed Apply did not restore healthy YAML")
            dns_query(dns_port, "192.0.2.44")
            checks.append("assembly failure restores disk and previous runtime")

            sidecars = {path.name for path in run.iterdir() if path.name.startswith(".config-")}
            require(".config-history.json" in sidecars and ".config-transaction.last.json" in sidecars, "generic sidecars missing")
            require(not any("standard" in name for name in sidecars), "mode-specific sidecar was created")
            checks.append("mode-neutral transaction sidecars")

            print(json.dumps({"ok": True, "checks": checks, "binary_sha256": args.expected_sha256}, indent=2))
            return 0
        finally:
            stop_process(process)
            mock.close()
            log.close()


if __name__ == "__main__":
    raise SystemExit(main())
