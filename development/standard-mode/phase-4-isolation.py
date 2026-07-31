#!/usr/bin/env python3
"""Credential-free, loopback-only Standard Mode Phase 4 acceptance harness."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import signal
import shutil
import socket
import struct
import subprocess
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


FORBIDDEN = ("openwrt", "uci", "ipset", "nftset", "routeros", "mikrotik")


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


def question_end(packet: bytes) -> int:
    offset = 12
    while offset < len(packet) and packet[offset] != 0:
        offset += 1 + packet[offset]
    if offset + 5 > len(packet):
        raise ValueError("truncated DNS question")
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


def dns_query(port: int, name: str, expected: str) -> None:
    labels = b"".join(bytes([len(label)]) + label.encode() for label in name.split("."))
    packet = struct.pack("!HHHHHH", 0x4F58, 0x0100, 1, 0, 0, 0) + labels + b"\0" + struct.pack("!HH", 1, 1)
    last_error: Exception | None = None
    for _ in range(40):
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
                sock.settimeout(0.5)
                sock.sendto(packet, ("127.0.0.1", port))
                response, _ = sock.recvfrom(4096)
            if socket.inet_aton(expected) not in response:
                raise RuntimeError("unexpected DNS answer")
            return
        except (OSError, RuntimeError) as error:
            last_error = error
            time.sleep(0.1)
    raise RuntimeError(f"DNS query failed: {last_error}")


def request(base: str, method: str, path: str, body: dict | None = None) -> tuple[int, dict]:
    data = None if body is None else json.dumps(body, separators=(",", ":")).encode()
    req = urllib.request.Request(
        base + path,
        data=data,
        method=method,
        headers={"Accept": "application/json", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as response:
            return int(response.status), json.loads(response.read())
    except urllib.error.HTTPError as error:
        return int(error.code), json.loads(error.read())


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def wait_api(base: str) -> None:
    last: object = None
    for _ in range(100):
        try:
            status, body = request(base, "GET", "/build")
            if status == 200 and body.get("ok"):
                return
            last = body
        except Exception as error:
            last = error
        time.sleep(0.1)
    raise RuntimeError(f"API did not become ready: {last}")


def stop_process(process: subprocess.Popen[bytes] | None) -> None:
    if process is None or process.poll() is not None:
        return
    process.send_signal(signal.SIGTERM)
    try:
        process.wait(timeout=8)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def make_intent(dns_port: int, upstream_port: int) -> dict:
    return {
        "schema": 6,
        "listen": {"address": f"127.0.0.1:{dns_port}", "udp": True, "tcp": True},
        "upstreamGroups": [{
            "id": "default",
            "name": "Phase 4 loopback",
            "strategy": "balanced",
            "isDefault": True,
            "upstreams": [{
                "id": "loopback",
                "name": "Loopback mock",
                "protocol": "udp",
                "address": f"127.0.0.1:{upstream_port}",
                "enabled": True,
                "tlsVerify": True,
            }],
        }],
        "paths": [{"id": "default", "name": "Default path", "upstreamGroupId": "default"}],
        "queryLog": {"enabled": True, "retentionDays": 1, "sampleRate": 1.0},
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default="./oxidns-linux-x86_64-musl")
    parser.add_argument("--expected-sha256", required=True)
    args = parser.parse_args()
    artifact_dir = Path(__file__).resolve().parent
    binary = (artifact_dir / args.binary).resolve()
    actual_hash = sha256(binary)
    require(actual_hash == args.expected_sha256, "binary hash mismatch")
    run = artifact_dir / "run"
    if run.exists():
        shutil.rmtree(run)
    run.mkdir(mode=0o700)

    api_port = reserve_port(socket.SOCK_STREAM)
    dns_port = reserve_port(socket.SOCK_DGRAM)
    mock = MockDns("192.0.2.44")
    mock.thread.start()
    config = run / "config.yaml"
    config.write_text(f"log:\n  level: warn\napi:\n  http:\n    listen: 127.0.0.1:{api_port}\nplugins: []\n", encoding="utf-8")
    log = (run / "oxidns.log").open("wb")
    process: subprocess.Popen[bytes] | None = None
    checks: list[str] = []
    result: dict = {"ok": False, "phase": 4, "checks": checks}
    try:
        process = subprocess.Popen(
            [str(binary), "start", "-c", str(config), "-d", str(run), "-l", "warn"],
            stdout=log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        base = f"http://127.0.0.1:{api_port}/api"
        wait_api(base)
        _, build = request(base, "GET", "/build")
        require(build["build"]["bundle"] == "standard", "binary is not the Standard bundle")
        checks.append("standard bundle")

        intent = make_intent(dns_port, mock.port)
        status, plan = request(base, "POST", "/standard/plan", {"intent": intent, "takeover": True})
        require(status == 200 and plan["can_apply"], f"Plan failed: {plan}")
        generated = plan["plan"]["generated"]
        explanation = generated["explanation"]
        revision = explanation["intentRevision"]
        require(revision.startswith("sha256:"), "intent revision is not canonical SHA-256")
        require(explanation["schema"] == 1 and explanation["pathBoundaries"], "explanation missing")
        require(explanation["pathBoundaries"][0]["upstreamMemberIds"] == ["loopback"], "stable member ID missing")
        require(plan["dependency_graph"]["nodes"], "dependency graph missing")
        require(plan["semantic_diff"]["affected"]["paths"], "semantic path impact missing")
        yaml_text = generated["yaml"]
        require("max_steps: 512" in yaml_text and "intentRevision" in yaml_text, "bounded recorder context missing")
        require(not any(term in yaml_text.lower() for term in FORBIDDEN), "forbidden integration generated")
        checks.append("explanation, graph, semantic impact, and native boundary")

        config_before_read_only = sha256(config)
        status, expert = request(base, "POST", "/standard/assets/expert-copy", {"intent": plan["plan"]["normalizedIntent"]})
        require(status == 200 and expert["detached"] and expert["intentRevision"] == revision, "Expert copy failed")
        status, analysis = request(base, "POST", "/standard/assets/expert-analysis", {"yaml": expert["yaml"]})
        require(status == 200 and analysis["readOnly"] and not analysis["reverseConversion"]["available"], "Expert analysis failed")
        require(sha256(config) == config_before_read_only, "read-only Expert bridge changed config")
        checks.append("detached Expert copy and read-only analysis")

        _, store = request(base, "GET", "/standard/assets/templates")
        template = {
            "id": "phase4_saved",
            "name": "Phase 4 saved",
            "kind": "low_latency",
            "parameters": {
                "namespace": "phase4_saved",
                "name": "Phase 4 saved",
                "domains": ["full:saved.phase4.test"],
                "upstreams": intent["upstreamGroups"][0]["upstreams"],
            },
            "sourceIntentSchema": 6,
            "createdAtMs": 0,
            "updatedAtMs": 0,
        }
        status, saved = request(base, "POST", "/standard/assets/templates", {"expectedVersion": store["store"]["version"], "template": template})
        require(status == 200 and len(saved["store"]["templates"]) == 1, "template save failed")
        status, duplicated = request(base, "POST", "/standard/assets/templates/duplicate", {
            "expectedVersion": saved["store"]["version"], "id": "phase4_saved", "newId": "phase4_saved_copy", "newName": "Phase 4 saved copy"
        })
        require(status == 200 and len(duplicated["store"]["templates"]) == 2, "template duplicate failed")
        stale_status, _ = request(base, "DELETE", "/standard/assets/templates", {"expectedVersion": saved["store"]["version"], "id": "phase4_saved"})
        require(stale_status == 409, "stale template mutation was not rejected")
        checks.append("bounded local template CRUD and optimistic conflict")

        status, accepted = request(base, "POST", "/standard/apply", {
            "intent": plan["plan"]["normalizedIntent"],
            "base_config_version": plan["config_version"],
            "base_standard_version": plan["standard_version"],
            "planned_config_version": generated["configVersion"],
            "takeover": True,
        })
        require(status == 202, f"Apply failed: {accepted}")
        transaction_id = accepted["transaction_id"]
        for _ in range(120):
            try:
                wait_api(base)
                _, transaction = request(base, "GET", "/standard/apply/status")
                current = transaction.get("transaction")
                if current and current.get("transaction_id") == transaction_id and current.get("status") == "succeeded":
                    break
            except (OSError, urllib.error.URLError):
                pass
            time.sleep(0.2)
        else:
            raise RuntimeError("transaction did not succeed")
        dns_query(dns_port, "diagnostic.phase4.test", "192.0.2.44")
        recorder = generated["tagMap"]["queryLog"]
        encoded = urllib.parse.quote(recorder, safe="")
        detail = None
        for _ in range(60):
            _, rows = request(base, "GET", f"/plugins/{encoded}/records?limit=20")
            if rows.get("records"):
                record_id = rows["records"][0]["id"]
                _, response = request(base, "GET", f"/plugins/{encoded}/records/{record_id}")
                detail = response["record"]
                break
            time.sleep(0.2)
        require(detail is not None, "query diagnostic record missing")
        diagnosis = detail["diagnosis"]
        require(diagnosis["intentRevision"] == revision and not diagnosis["explanationUnavailable"], "query revision mismatch")
        require(not diagnosis["stepsTruncated"] and diagnosis["droppedStepCount"] == 0, "unexpected trace truncation")
        events = detail["steps"]
        require(any(item["kind"] == "cache" for item in events), "cache decision event missing")
        require(any(item["kind"] == "upstream" and item["outcome"] == "selected" for item in events), "selected upstream event missing")
        require(len(events) <= 512 and mock.requests > 0, "trace bound or loopback upstream failed")
        checks.append("transactional Apply and revision-pinned query diagnosis")

        config_after_apply = sha256(config)
        status, exported = request(base, "GET", "/standard/assets/export")
        require(status == 200 and exported["asset"]["intentRevision"] == revision, "asset export failed")
        status, imported = request(base, "POST", "/standard/assets/import", {"asset": exported["asset"]})
        require(status == 200 and imported["intent_revision"] == revision, "asset round trip failed")
        legacy = {
            "assetSchema": 1, "kind": "oxidns_standard_intent", "oxidnsVersion": "legacy", "bundle": "standard",
            "intentSchema": 1, "intentRevision": "legacy-untrusted", "exportedAtMs": 1,
            "intent": {"schema": 1, "listen": {"address": f"127.0.0.1:{dns_port}", "udp": True, "tcp": True},
                       "upstreams": [{"id": "legacy", "name": "旧版上游", "address": f"127.0.0.1:{mock.port}", "enabled": True}]},
        }
        status, migrated = request(base, "POST", "/standard/assets/import", {"asset": legacy, "takeover": True})
        require(status == 200 and migrated["source_intent_schema"] == 1 and migrated["plan"]["plan"]["normalizedIntent"]["schema"] == 6, "legacy migration failed")
        require(sha256(config) == config_after_apply, "asset import changed runtime config")
        checks.append("portable asset round trip and schema-v1 migration without write")

        _, history = request(base, "GET", "/standard/history")
        require(history["entries"] and history["entries"][0]["intent_revision"] == revision, "history revision missing")
        entry_id = history["entries"][0]["id"]
        status, restored = request(base, "POST", "/standard/history/restore", {"id": entry_id})
        require(status == 200 and restored["entry"]["settings"]["schema"] == 6, "restore preview failed")
        require(sha256(config) == config_after_apply, "restore preview changed runtime config")
        checks.append("successful history and read-only restore preview")

        result = {
            "ok": True,
            "phase": 4,
            "bundle": "standard",
            "binarySha256": actual_hash,
            "intentRevision": revision,
            "loopbackOnly": True,
            "credentialsEmbedded": False,
            "forbiddenIntegrationScan": "passed",
            "exactChildCleanup": "pending-finally",
            "checks": checks,
            "queryEventCount": len(events),
            "upstreamRequests": mock.requests,
        }
        return 0
    except Exception as error:
        result["error"] = str(error)
        return 1
    finally:
        stop_process(process)
        log.close()
        mock.close()
        if result.get("ok"):
            result["exactChildCleanup"] = "passed"
        (artifact_dir / "result.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    raise SystemExit(main())
