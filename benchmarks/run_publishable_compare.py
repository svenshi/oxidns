#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Sven Shi
# SPDX-License-Identifier: GPL-3.0-or-later

"""Run the publishable multi-engine DNS benchmark matrix.

The runner intentionally uses only Python's standard library.  It requires a
Linux host and a dnsperf build with JSON and latency-histogram support.
"""

from __future__ import annotations

import argparse
import hashlib
import html
import json
import math
import os
import platform
import re
import shutil
import signal
import socket
import statistics
import struct
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable


BASE_DIR = Path(__file__).resolve().parent
REPO_DIR = BASE_DIR.parent
DEFAULT_SCENARIOS = (
    "02-cache-hotpath",
    "47-server-local-udp",
    "48-server-local-tcp",
    "50-common-local-answers",
    "51-common-domain-set",
    "52-negative-cache-hotpath",
)
NATIVE_SPECIALIZED_SCENARIOS = (
    "08-domain-set",
    "09-ip-set",
    "42-composite-local-rewrite",
    "43-composite-provider-chain",
)
OXIDNS_FEATURE_SCENARIOS = ("60-cache-short-circuit", "61-cache-explicit-accept")
CACHE_FIXTURE_SCENARIOS = (
    "02-cache-hotpath",
    "52-negative-cache-hotpath",
    *OXIDNS_FEATURE_SCENARIOS,
)
ENGINES = ("oxidns", "mosdns", "adguardhome", "smartdns")
ENGINE_LABELS = {
    "oxidns": "OxiDNS",
    "mosdns": "mosdns",
    "adguardhome": "AdGuard Home",
    "smartdns": "SmartDNS",
}
ENGINE_COLORS = {
    "oxidns": "#0f766e",
    "mosdns": "#f59e0b",
    "adguardhome": "#2563eb",
    "smartdns": "#dc2626",
}
COMMON_DOMAIN_SOURCES = (
    BASE_DIR / "data" / "geosite_cn.txt",
    BASE_DIR / "data" / "geosite_geolocation-!cn.txt",
)
COMMON_DOMAIN_DIR = BASE_DIR / ".generated"
COMMON_DOMAIN_LIST = COMMON_DOMAIN_DIR / "common-domains.txt"
COMMON_DOMAIN_ANSWERS = COMMON_DOMAIN_DIR / "common-domain-hit-answers.zone"
COMMON_DOMAIN_HIT_IP = "192.0.2.53"
COMMON_DOMAIN_MISS_IP = "192.0.2.54"
FIXTURE_HOST = "127.0.0.1"
FIXTURE_PORT = 5453
FIXTURE_POSITIVE_IP = "192.0.2.100"
SEMANTIC_PROBES = {
    "02-cache-hotpath": (("www.baidu.com", "192.0.2.100"),),
    "47-server-local-udp": (("bench.test", "192.0.2.10"),),
    "48-server-local-tcp": (("bench.test", "192.0.2.10"),),
    "50-common-local-answers": (
        ("bench.local", "192.0.2.10"),
        ("svc.bench.local", "192.0.2.20"),
        ("api.bench.local", "192.0.2.40"),
        ("cdn.bench.local", "192.0.2.50"),
        ("metrics.bench.local", "192.0.2.60"),
    ),
    "51-common-domain-set": (
        ("265.com", COMMON_DOMAIN_HIT_IP),
        ("semantic-miss.invalid", COMMON_DOMAIN_MISS_IP),
    ),
    "43-composite-provider-chain": (
        ("265.com", "10.1.0.10"),
        ("apps.mzstatic.com", "10.2.0.20"),
        ("azure.microsoft.com", "198.51.100.10"),
    ),
    "09-ip-set": (
        ("ipset-01.bench.test", "10.1.0.10"),
        ("ipset-02.bench.test", "10.2.0.20"),
        ("ipset-03.bench.test", "10.3.0.30"),
        ("ipset-04.bench.test", "10.4.0.40"),
    ),
    "42-composite-local-rewrite": (
        ("bench.test", "192.0.2.10"),
        ("bench-alt.test", "192.0.2.11"),
    ),
    "60-cache-short-circuit": (("www.baidu.com", "192.0.2.100"),),
    "61-cache-explicit-accept": (("www.baidu.com", "192.0.2.100"),),
}
SEMANTIC_RCODE_PROBES = {
    "08-domain-set": (
        ("265.com", 5),
        ("2mdn-cn.net", 5),
        ("a1.mzstatic.com", 5),
        ("adcdownload.apple.com", 5),
        ("apps.mzstatic.com", 5),
        ("build.microsoft.com", 5),
        ("1password.drift.click", 5),
        ("3dns.adobe.com", 5),
        ("android.googlesource.com", 5),
        ("azure.microsoft.com", 5),
        ("sub.jsxcra.com", 5),
        ("13mei5.buzz", 5),
        *((f"bench-miss-{index:02d}.example", 2) for index in range(1, 9)),
    ),
    "43-composite-provider-chain": (
        ("android.googlesource.com", 2),
        *((f"bench-provider-miss-{index:02d}.example", 2) for index in range(1, 5)),
    ),
    "09-ip-set": tuple((f"ipset-{index:02d}.bench.test", 2) for index in range(5, 9)),
    "52-negative-cache-hotpath": (("negative-cache-01.bench.invalid", 3),),
}
SEMANTIC_AAAA_PROBES = {
    "42-composite-local-rewrite": (("bench-v6.test", "2001:db8::10"),),
    "50-common-local-answers": (
        ("bench.local", "2001:db8::10"),
        ("svc.bench.local", "2001:db8::20"),
        ("api.bench.local", "2001:db8::40"),
        ("cdn.bench.local", "2001:db8::50"),
        ("metrics.bench.local", "2001:db8::60"),
    ),
}
SEMANTIC_TTL_PROBES = {
    ("02-cache-hotpath", "www.baidu.com", "A"): 300,
    ("47-server-local-udp", "bench.test", "A"): 10,
    ("48-server-local-tcp", "bench.test", "A"): 10,
    **{("50-common-local-answers", name, "A"): 10 for name in (
        "bench.local", "svc.bench.local", "api.bench.local", "cdn.bench.local", "metrics.bench.local",
    )},
    **{("50-common-local-answers", name, "AAAA"): 10 for name in (
        "bench.local", "svc.bench.local", "api.bench.local", "cdn.bench.local", "metrics.bench.local",
    )},
    ("51-common-domain-set", "265.com", "A"): 60,
    ("51-common-domain-set", "semantic-miss.invalid", "A"): 60,
    ("42-composite-local-rewrite", "bench.test", "A"): 60,
    ("42-composite-local-rewrite", "bench-alt.test", "A"): 60,
    ("42-composite-local-rewrite", "bench-v6.test", "AAAA"): 60,
    ("60-cache-short-circuit", "www.baidu.com", "A"): 300,
    ("61-cache-explicit-accept", "www.baidu.com", "A"): 300,
}
SEMANTIC_SOA_PROBES = {
    ("52-negative-cache-hotpath", "negative-cache-01.bench.invalid"): (300, 300),
}
DOMAIN_LABEL_RE = re.compile(r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$")


@dataclass(frozen=True)
class Scenario:
    label: str
    engines: tuple[str, ...]
    configs: dict[str, Path]
    query_file: Path
    mode: str
    family: str
    warmup_file: Path
    tags: tuple[str, ...]
    description: str
    notes: str

    def config_for(self, engine: str) -> Path:
        try:
            return self.configs[engine]
        except KeyError as exc:
            raise ValueError(f"{self.label} does not support {engine}") from exc


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run reproducible QPS, tail-latency, CPU and RSS comparisons."
    )
    parser.add_argument("selectors", nargs="*", help="scenario labels, families, tags, or all")
    parser.add_argument("--engines", default=os.getenv("BENCH_ENGINES", ",".join(ENGINES)), help="comma-separated engines")
    parser.add_argument("--load-levels", default=os.getenv("BENCH_LOAD_LEVELS", "1,4,16,64,256,1024"))
    parser.add_argument("--seconds", type=int, default=int(os.getenv("BENCH_SECONDS", "12")))
    parser.add_argument("--warmup-seconds", type=int, default=int(os.getenv("WARMUP_SECONDS", "3")))
    parser.add_argument("--repeats", type=int, default=int(os.getenv("BENCH_REPEATS", "3")))
    parser.add_argument("--threads", type=int, default=int(os.getenv("DNSPERF_THREADS", "4")))
    parser.add_argument("--max-clients", type=int, default=int(os.getenv("DNSPERF_MAX_CLIENTS", "32")))
    parser.add_argument("--timeout", type=int, default=int(os.getenv("DNSPERF_TIMEOUT", "5")))
    parser.add_argument("--sample-interval", type=float, default=float(os.getenv("RESOURCE_SAMPLE_INTERVAL", "0.2")))
    parser.add_argument("--cooldown", type=float, default=float(os.getenv("BENCH_COOLDOWN_SECONDS", "1")))
    parser.add_argument("--result-dir", type=Path)
    parser.add_argument("--publish-docs", action="store_true", help="replace docs current snapshot and chart assets")
    parser.add_argument(
        "--publish-native-specialized",
        action="store_true",
        help="publish the OxiDNS/mosdns native-rule suite beside, rather than over, the four-engine snapshot",
    )
    parser.add_argument("--publish-existing", type=Path, help="publish an already completed result directory")
    parser.add_argument("--dry-run", action="store_true", help="validate and print the matrix without starting servers")
    return parser.parse_args()


def positive_levels(raw: str) -> list[int]:
    try:
        levels = sorted({int(part.strip()) for part in raw.replace(" ", ",").split(",") if part.strip()})
    except ValueError as exc:
        raise SystemExit(f"invalid --load-levels: {raw}") from exc
    if not levels or levels[0] < 1:
        raise SystemExit("--load-levels must contain positive integers")
    return levels


def selected_engines(raw: str) -> list[str]:
    engines = [part.strip().lower() for part in raw.split(",") if part.strip()]
    unknown = sorted(set(engines) - set(ENGINES))
    if unknown:
        raise SystemExit(f"unknown engines: {', '.join(unknown)}")
    if not engines:
        raise SystemExit("--engines must select at least one engine")
    return list(dict.fromkeys(engines))


def load_catalog(path: Path) -> list[Scenario]:
    scenarios: list[Scenario] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        fields = line.split("|", 12)
        if len(fields) != 13:
            raise SystemExit(f"invalid scenario row: {line}")
        (
            label, engine_list, ox_cfg, mos_cfg, adguard_cfg, smartdns_cfg,
            query, mode, family, warmup, tags, description, notes,
        ) = fields
        engines = tuple(part.strip() for part in engine_list.split(",") if part.strip())
        unknown_engines = sorted(set(engines) - set(ENGINES))
        if not engines or unknown_engines:
            raise SystemExit(f"invalid engines for {label}: {engine_list}")
        raw_configs = {
            "oxidns": ox_cfg,
            "mosdns": mos_cfg,
            "adguardhome": adguard_cfg,
            "smartdns": smartdns_cfg,
        }
        configs = {
            engine: BASE_DIR / raw_configs[engine]
            for engine in engines
            if raw_configs[engine] not in ("", "-")
        }
        if set(configs) != set(engines):
            raise SystemExit(f"missing declared engine config for {label}")
        query_path = BASE_DIR / query
        warmup_path = query_path if warmup in ("", "-") else BASE_DIR / warmup
        scenarios.append(
            Scenario(
                label,
                engines,
                configs,
                query_path,
                mode,
                family,
                warmup_path,
                tuple(tags.split(",")),
                description,
                notes,
            )
        )
    return scenarios


def select_scenarios(catalog: list[Scenario], selectors: list[str]) -> list[Scenario]:
    wanted = selectors or list(DEFAULT_SCENARIOS)
    selected = [
        item
        for item in catalog
        if "all" in wanted
        or ("native-specialized" in wanted and item.label in NATIVE_SPECIALIZED_SCENARIOS)
        or ("oxidns-features" in wanted and item.label in OXIDNS_FEATURE_SCENARIOS)
        or item.label in wanted
        or item.family in wanted
        or any(tag in wanted for tag in item.tags)
    ]
    if not selected:
        raise SystemExit(f"no scenarios matched: {' '.join(wanted)}")
    return selected


def require_executable(value: str, label: str) -> str:
    resolved = shutil.which(value) if "/" not in value else value
    if not resolved or not os.access(resolved, os.X_OK):
        raise SystemExit(f"missing executable for {label}: {value}")
    return str(Path(resolved).resolve())


def command_output(command: list[str]) -> str:
    try:
        return subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False).stdout.strip()
    except OSError as exc:
        return f"unavailable: {exc}"


def binary_version(path: str, engine: str) -> str:
    commands = [[path, "--version"]]
    if engine == "mosdns":
        commands.insert(0, [path, "version"])
    elif engine == "smartdns":
        commands.insert(0, [path, "-v"])
    for command in commands:
        output = command_output(command).splitlines()
        if output:
            return output[0]
    return "n/a"


def dnsperf_version(path: str) -> str:
    output = command_output([path, "-h"])
    lines = [line.strip() for line in output.splitlines() if "Version" in line]
    return lines[-1] if lines else "recorded from dnsperf JSON at first measured run"


def git_value(*arguments: str) -> str:
    if not (REPO_DIR / ".git").exists():
        return "n/a (release artifact benchmark)"
    return command_output(["git", "-C", str(REPO_DIR), *arguments])


def sha256(path: str | Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def benchmark_input_manifest(
    scenarios: list[Scenario], scenario_engines: dict[str, list[str]],
) -> dict[str, str]:
    paths = {BASE_DIR / "scenarios.tsv", Path(__file__).resolve()}
    paths.update(path for path in (BASE_DIR / "data").glob("**/*") if path.is_file())
    for scenario in scenarios:
        paths.add(scenario.query_file)
        paths.add(scenario.warmup_file)
        paths.update(scenario.config_for(engine) for engine in scenario_engines[scenario.label])
    for generated in (COMMON_DOMAIN_LIST, COMMON_DOMAIN_ANSWERS):
        if generated.is_file():
            paths.add(generated)
    return {
        path.relative_to(BASE_DIR).as_posix(): sha256(path)
        for path in sorted(paths)
    }


def extract_listen(config: Path, engine: str, mode: str) -> tuple[str, int]:
    lines = config.read_text(encoding="utf-8").splitlines()
    if engine in ("oxidns", "mosdns"):
        for line in lines:
            stripped = line.strip()
            if stripped.startswith("listen:"):
                value = stripped.split(":", 1)[1].strip().strip('"').strip("'")
                host, port = value.rsplit(":", 1)
                return host, int(port)
    elif engine == "adguardhome":
        host = next(
            (
                match.group(1)
                for line in lines
                if (match := re.search(r"bind_hosts:\s*\[\s*['\"]?([^,'\"\]\s]+)", line))
            ),
            None,
        )
        port = next(
            (int(match.group(1)) for line in lines if (match := re.match(r"\s*port:\s*(\d+)\s*$", line))),
            None,
        )
        if host is not None and port is not None:
            return host, port
    elif engine == "smartdns":
        directive = "bind-tcp" if mode == "tcp" else "bind"
        for line in lines:
            fields = line.strip().split()
            if len(fields) >= 2 and fields[0] == directive:
                host, port = fields[1].rsplit(":", 1)
                return host, int(port)
    raise SystemExit(f"{mode} listen address for {engine} not found in {config}")


def percentile_from_histogram(histogram: list[list[float]], percentile: float) -> float | None:
    total = sum(int(bucket[2]) for bucket in histogram)
    if not total:
        return None
    target = math.ceil(total * percentile)
    seen = 0
    for _lower, upper, count in histogram:
        seen += int(count)
        if seen >= target:
            return float(upper) * 1000.0
    return float(histogram[-1][1]) * 1000.0


class ProcSampler:
    def __init__(self, pid: int, interval: float, output: Path) -> None:
        self.pid = pid
        self.interval = interval
        self.output = output
        self.stop_event = threading.Event()
        self.samples: list[dict[str, float]] = []
        self.thread = threading.Thread(target=self._run, daemon=True)

    def start(self) -> None:
        self.thread.start()

    def stop(self) -> dict[str, float]:
        self.stop_event.set()
        self.thread.join(timeout=max(2.0, self.interval * 4))
        with self.output.open("w", encoding="utf-8") as stream:
            stream.write("elapsed_s\tcpu_pct\trss_mib\tthreads\n")
            for item in self.samples:
                stream.write(f"{item['elapsed']:.6f}\t{item['cpu']:.3f}\t{item['rss']:.3f}\t{item['threads']:.0f}\n")
        cpus = [item["cpu"] for item in self.samples[1:]]
        rss = [item["rss"] for item in self.samples]
        threads = [item["threads"] for item in self.samples]
        if len(cpus) < 2 or not rss or max(rss) <= 0:
            raise RuntimeError(f"insufficient resource samples for pid {self.pid}; see {self.output}")
        return {
            "cpu_pct_median": median(cpus),
            "cpu_pct_p95": quantile(cpus, 0.95),
            "rss_mib_median": median(rss),
            "rss_mib_max": max(rss, default=0.0),
            "threads_max": max(threads, default=0.0),
            "resource_samples": float(len(self.samples)),
        }

    def _run(self) -> None:
        ticks = os.sysconf(os.sysconf_names["SC_CLK_TCK"])
        started = time.monotonic()
        previous_time: float | None = None
        previous_ticks: int | None = None
        while not self.stop_event.is_set():
            now = time.monotonic()
            try:
                stat = Path(f"/proc/{self.pid}/stat").read_text().split()
                status = Path(f"/proc/{self.pid}/status").read_text().splitlines()
            except (FileNotFoundError, ProcessLookupError):
                break
            total_ticks = int(stat[13]) + int(stat[14])
            values: dict[str, str] = {}
            for line in status:
                if ":" in line:
                    key, value = line.split(":", 1)
                    fields = value.strip().split()
                    if fields:
                        values[key] = fields[0]
            cpu = 0.0
            if previous_time is not None and previous_ticks is not None and now > previous_time:
                cpu = ((total_ticks - previous_ticks) / ticks) / (now - previous_time) * 100.0
            self.samples.append(
                {
                    "elapsed": now - started,
                    "cpu": cpu,
                    "rss": float(values.get("VmRSS", "0")) / 1024.0,
                    "threads": float(values.get("Threads", "0")),
                }
            )
            previous_time, previous_ticks = now, total_ticks
            self.stop_event.wait(self.interval)


def median(values: Iterable[float]) -> float:
    items = list(values)
    return float(statistics.median(items)) if items else 0.0


def quantile(values: Iterable[float], q: float) -> float:
    items = sorted(values)
    if not items:
        return 0.0
    return float(items[min(len(items) - 1, max(0, math.ceil(len(items) * q) - 1))])


def stop_process(process: subprocess.Popen[str] | None) -> None:
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=2)


def common_domain(raw: str) -> tuple[str | None, str]:
    value = raw.strip().lower().rstrip(".")
    if not value or value.startswith(("#", "!")):
        return None, "ignored"
    kind = "plain"
    if ":" in value:
        kind, value = value.split(":", 1)
    if kind not in ("plain", "full", "domain"):
        return None, f"unsupported_{kind}"
    labels = value.split(".")
    if len(labels) < 2 or len(value) > 253 or any(not DOMAIN_LABEL_RE.fullmatch(label) for label in labels):
        return None, "invalid_domain"
    return value, kind


def prepare_common_domain_assets(query_file: Path) -> dict[str, int]:
    domains: set[str] = set()
    stats: dict[str, int] = {}
    for source in COMMON_DOMAIN_SOURCES:
        for raw in source.read_text(encoding="utf-8").splitlines():
            domain, kind = common_domain(raw)
            if domain is None:
                if kind != "ignored":
                    stats[kind] = stats.get(kind, 0) + 1
                continue
            if domain in domains:
                stats["duplicate"] = stats.get("duplicate", 0) + 1
                continue
            domains.add(domain)
            key = f"included_{kind}"
            stats[key] = stats.get(key, 0) + 1

    query_domains: list[str] = []
    for line in query_file.read_text(encoding="utf-8").splitlines():
        fields = line.split()
        if not fields:
            continue
        if len(fields) != 2 or fields[1].upper() != "A":
            raise RuntimeError(f"common domain query must be an A query: {line}")
        domain = fields[0].lower().rstrip(".")
        if domain not in domains:
            raise RuntimeError(f"common domain query is absent from normalized corpus: {domain}")
        query_domains.append(domain)
    if not query_domains:
        raise RuntimeError("common domain query file is empty")

    COMMON_DOMAIN_DIR.mkdir(parents=True, exist_ok=True)
    COMMON_DOMAIN_LIST.write_text("\n".join(sorted(domains)) + "\n", encoding="utf-8")
    COMMON_DOMAIN_ANSWERS.write_text(
        "".join(f"{domain}. 60 IN A {COMMON_DOMAIN_HIT_IP}\n" for domain in query_domains),
        encoding="utf-8",
    )
    stats["normalized_unique_domains"] = len(domains)
    stats["positive_query_domains"] = len(query_domains)
    return stats


def encode_dns_name(name: str) -> bytes:
    return b"".join(bytes((len(label),)) + label.encode("ascii") for label in name.rstrip(".").split(".")) + b"\0"


def skip_dns_name(message: bytes, offset: int) -> int:
    while True:
        if offset >= len(message):
            raise RuntimeError("truncated DNS name")
        length = message[offset]
        if length & 0xC0 == 0xC0:
            return offset + 2
        offset += 1
        if length == 0:
            return offset
        offset += length


def recv_exact(stream: socket.socket, length: int) -> bytes:
    chunks: list[bytes] = []
    remaining = length
    while remaining:
        chunk = stream.recv(remaining)
        if not chunk:
            raise RuntimeError("truncated TCP DNS response")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def query_dns(host: str, port: int, name: str, query_type: str = "A", transport: str = "udp") -> dict[str, Any]:
    query_types = {"A": 1, "AAAA": 28}
    record_type = query_types[query_type]
    query_id = (os.getpid() ^ sum(name.encode("ascii")) ^ record_type) & 0xFFFF
    packet = (
        struct.pack("!HHHHHH", query_id, 0x0100, 1, 0, 0, 0)
        + encode_dns_name(name)
        + struct.pack("!HH", record_type, 1)
    )
    family = socket.AF_INET6 if ":" in host else socket.AF_INET
    if transport == "tcp":
        with socket.socket(family, socket.SOCK_STREAM) as stream:
            stream.settimeout(2.0)
            stream.connect((host, port))
            stream.sendall(struct.pack("!H", len(packet)) + packet)
            response_length = struct.unpack("!H", recv_exact(stream, 2))[0]
            response = recv_exact(stream, response_length)
    else:
        with socket.socket(family, socket.SOCK_DGRAM) as stream:
            stream.settimeout(2.0)
            stream.sendto(packet, (host, port))
            response, _peer = stream.recvfrom(65535)
    if len(response) < 12:
        raise RuntimeError(f"truncated DNS response for {name}")
    response_id, flags, questions, answers, authority_count, _additional = struct.unpack("!HHHHHH", response[:12])
    if response_id != query_id:
        raise RuntimeError(f"DNS response ID mismatch for {name}")
    offset = 12
    for _ in range(questions):
        offset = skip_dns_name(response, offset) + 4
    a_records: list[str] = []
    aaaa_records: list[str] = []
    records: list[dict[str, Any]] = []
    for _ in range(answers):
        offset = skip_dns_name(response, offset)
        if offset + 10 > len(response):
            raise RuntimeError(f"truncated DNS answer for {name}")
        answer_type, record_class, ttl, data_len = struct.unpack("!HHIH", response[offset:offset + 10])
        offset += 10
        data = response[offset:offset + data_len]
        offset += data_len
        if answer_type == 1 and record_class == 1 and data_len == 4:
            address = socket.inet_ntop(socket.AF_INET, data)
            a_records.append(address)
            records.append({"type": "A", "address": address, "ttl": ttl})
        elif answer_type == 28 and record_class == 1 and data_len == 16:
            address = socket.inet_ntop(socket.AF_INET6, data)
            aaaa_records.append(address)
            records.append({"type": "AAAA", "address": address, "ttl": ttl})
    authority_records: list[dict[str, Any]] = []
    for _ in range(authority_count):
        offset = skip_dns_name(response, offset)
        if offset + 10 > len(response):
            raise RuntimeError(f"truncated DNS authority record for {name}")
        authority_type, record_class, ttl, data_len = struct.unpack("!HHIH", response[offset:offset + 10])
        offset += 10
        data_offset = offset
        data_end = data_offset + data_len
        if data_end > len(response):
            raise RuntimeError(f"truncated DNS authority data for {name}")
        record: dict[str, Any] = {"type_code": authority_type, "class": record_class, "ttl": ttl}
        if authority_type == 6 and record_class == 1:
            soa_offset = skip_dns_name(response, data_offset)
            soa_offset = skip_dns_name(response, soa_offset)
            if soa_offset + 20 > data_end:
                raise RuntimeError(f"truncated DNS SOA record for {name}")
            serial, refresh, retry, expire, minimum = struct.unpack("!IIIII", response[soa_offset:soa_offset + 20])
            record.update({
                "type": "SOA",
                "serial": serial,
                "refresh": refresh,
                "retry": retry,
                "expire": expire,
                "minimum": minimum,
            })
        authority_records.append(record)
        offset = data_end
    return {
        "name": name,
        "query_type": query_type,
        "transport": transport,
        "rcode": flags & 0x0F,
        "a_records": a_records,
        "aaaa_records": aaaa_records,
        "records": records,
        "authority_records": authority_records,
    }


def decode_dns_question(packet: bytes) -> tuple[str, int, int, bytes]:
    if len(packet) < 17:
        raise ValueError("truncated DNS query")
    labels: list[str] = []
    offset = 12
    while True:
        if offset >= len(packet):
            raise ValueError("truncated DNS query name")
        length = packet[offset]
        offset += 1
        if length == 0:
            break
        if length & 0xC0 or offset + length > len(packet):
            raise ValueError("unsupported DNS query name")
        labels.append(packet[offset:offset + length].decode("ascii"))
        offset += length
    if offset + 4 > len(packet):
        raise ValueError("truncated DNS question")
    query_type, query_class = struct.unpack("!HH", packet[offset:offset + 4])
    return ".".join(labels).lower(), query_type, query_class, packet[12:offset + 4]


class DnsFixture:
    """Small deterministic UDP authority used only to fill benchmark caches."""

    def __init__(self, host: str = FIXTURE_HOST, port: int = FIXTURE_PORT) -> None:
        self.host = host
        self.port = port
        self._socket: socket.socket | None = None
        self._thread: threading.Thread | None = None
        self._stop = threading.Event()
        self._lock = threading.Lock()
        self._requests = 0

    @property
    def requests(self) -> int:
        with self._lock:
            return self._requests

    def start(self) -> None:
        stream = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        stream.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        stream.bind((self.host, self.port))
        stream.settimeout(0.2)
        self._socket = stream
        self._thread = threading.Thread(target=self._run, name="benchmark-dns-fixture", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._socket is not None:
            self._socket.close()
        if self._thread is not None:
            self._thread.join(timeout=2.0)

    def _run(self) -> None:
        assert self._socket is not None
        while not self._stop.is_set():
            try:
                packet, peer = self._socket.recvfrom(65535)
            except socket.timeout:
                continue
            except OSError:
                break
            try:
                response = self._answer(packet)
            except (UnicodeDecodeError, ValueError, struct.error):
                continue
            with self._lock:
                self._requests += 1
            try:
                self._socket.sendto(response, peer)
            except OSError:
                break

    @staticmethod
    def _answer(packet: bytes) -> bytes:
        query_id, query_flags, questions, _answers, _authority, _additional = struct.unpack("!HHHHHH", packet[:12])
        if questions != 1:
            raise ValueError("fixture requires one DNS question")
        name, query_type, query_class, question = decode_dns_question(packet)
        response_flags = 0x8000 | (query_flags & 0x0100) | 0x0080
        if name.endswith(".bench.invalid"):
            header = struct.pack("!HHHHHH", query_id, response_flags | 3, 1, 0, 1, 0)
            soa_data = (
                encode_dns_name("ns.bench.invalid")
                + encode_dns_name("hostmaster.bench.invalid")
                + struct.pack("!IIIII", 1, 60, 60, 60, 300)
            )
            authority = b"\xc0\x0c" + struct.pack("!HHIH", 6, 1, 300, len(soa_data)) + soa_data
            return header + question + authority
        if query_class == 1 and query_type == 1:
            header = struct.pack("!HHHHHH", query_id, response_flags, 1, 1, 0, 0)
            address = socket.inet_pton(socket.AF_INET, FIXTURE_POSITIVE_IP)
            answer = b"\xc0\x0c" + struct.pack("!HHIH", 1, 1, 300, len(address)) + address
            return header + question + answer
        header = struct.pack("!HHHHHH", query_id, response_flags, 1, 0, 0, 0)
        return header + question


def valid_ttl(record_ttl: int, expected_ttl: int | None, age_tolerant: bool) -> bool:
    if expected_ttl is None:
        return True
    if age_tolerant:
        return max(0, expected_ttl - 15) <= record_ttl <= expected_ttl
    return record_ttl == expected_ttl


def validate_address_result(
    result: dict[str, Any], scenario: Scenario, engine: str, name: str,
    query_type: str, expected_ip: str, cache_recheck: bool,
) -> None:
    expected_ttl = SEMANTIC_TTL_PROBES.get((scenario.label, name, query_type))
    matching_records = [
        record for record in result["records"]
        if record["type"] == query_type and record["address"] == expected_ip
    ]
    if result["rcode"] != 0 or not matching_records or not any(
        valid_ttl(
            int(record["ttl"]),
            expected_ttl,
            cache_recheck or scenario.label in CACHE_FIXTURE_SCENARIOS,
        )
        for record in matching_records
    ):
        raise RuntimeError(
            f"semantic probe failed for {scenario.label}/{engine}: {name} expected {query_type} {expected_ip}, "
            f"ttl={expected_ttl}, cache_recheck={cache_recheck}, got rcode={result['rcode']} "
            f"records={result['records']}"
        )
    result[f"expected_{query_type.lower()}"] = expected_ip
    if expected_ttl is not None:
        result["expected_ttl"] = expected_ttl
    if cache_recheck:
        result["cache_recheck"] = True


def validate_rcode_result(
    result: dict[str, Any], scenario: Scenario, engine: str, name: str,
    expected_rcode: int, cache_recheck: bool,
) -> None:
    expected_soa = SEMANTIC_SOA_PROBES.get((scenario.label, name))
    soa_records = [record for record in result["authority_records"] if record.get("type") == "SOA"]
    soa_valid = expected_soa is None or any(
        valid_ttl(
            int(record["ttl"]),
            expected_soa[0],
            cache_recheck or scenario.label in CACHE_FIXTURE_SCENARIOS,
        )
        and int(record["minimum"]) == expected_soa[1]
        for record in soa_records
    )
    if result["rcode"] != expected_rcode or not soa_valid:
        raise RuntimeError(
            f"semantic probe failed for {scenario.label}/{engine}: {name} expected rcode={expected_rcode}, "
            f"soa={expected_soa}, cache_recheck={cache_recheck}, got rcode={result['rcode']} "
            f"authority={result['authority_records']}"
        )
    result["expected_rcode"] = expected_rcode
    if expected_soa is not None:
        result["expected_soa_ttl"] = expected_soa[0]
        result["expected_soa_minimum"] = expected_soa[1]
    if cache_recheck:
        result["cache_recheck"] = True


def validate_semantics(scenario: Scenario, engine: str, host: str, port: int) -> dict[str, Any]:
    results: list[dict[str, Any]] = []
    cache_rechecks = scenario.label in CACHE_FIXTURE_SCENARIOS
    for name, expected_ip in SEMANTIC_PROBES.get(scenario.label, ()):
        for cache_recheck in ((False, True) if cache_rechecks else (False,)):
            result = query_dns(host, port, name, transport=scenario.mode)
            validate_address_result(result, scenario, engine, name, "A", expected_ip, cache_recheck)
            results.append(result)
    for name, expected_ip in SEMANTIC_AAAA_PROBES.get(scenario.label, ()):
        for cache_recheck in ((False, True) if cache_rechecks else (False,)):
            result = query_dns(host, port, name, query_type="AAAA", transport=scenario.mode)
            validate_address_result(result, scenario, engine, name, "AAAA", expected_ip, cache_recheck)
            results.append(result)
    for name, expected_rcode in SEMANTIC_RCODE_PROBES.get(scenario.label, ()):
        for cache_recheck in ((False, True) if cache_rechecks else (False,)):
            result = query_dns(host, port, name, transport=scenario.mode)
            validate_rcode_result(result, scenario, engine, name, expected_rcode, cache_recheck)
            results.append(result)
    return {"scenario": scenario.label, "engine": engine, "probes": results}


def start_engine(engine: str, binary: str, config: Path, log_path: Path) -> tuple[subprocess.Popen[str], Any]:
    log_stream = log_path.open("w", encoding="utf-8")
    if engine == "oxidns":
        command = [binary, "start", "-c", str(config)]
    elif engine == "mosdns":
        help_text = command_output([binary, "--help"])
        command = [binary, "start", "-c", str(config)] if " start " in f" {help_text} " else [binary, "-c", str(config)]
    elif engine == "adguardhome":
        work_dir = log_path.parent / f"{log_path.stem}.work"
        work_dir.mkdir(parents=True, exist_ok=True)
        runtime_config = work_dir / "AdGuardHome.yaml"
        runtime_config.write_text(
            config.read_text(encoding="utf-8").replace("__BASE_DIR__", str(BASE_DIR)),
            encoding="utf-8",
        )
        if config.stem == "51-common-domain-set":
            filter_dir = work_dir / "data" / "filters"
            filter_dir.mkdir(parents=True, exist_ok=True)
            shutil.copy2(COMMON_DOMAIN_LIST, filter_dir / "1.txt")
        command = [binary, "--no-check-update", "--config", str(runtime_config), "--work-dir", str(work_dir)]
    elif engine == "smartdns":
        command = [binary, "-f", "-c", str(config)]
    else:
        log_stream.close()
        raise ValueError(f"unsupported engine: {engine}")
    process = subprocess.Popen(command, cwd=BASE_DIR, stdout=log_stream, stderr=subprocess.STDOUT, text=True)
    time.sleep(1.0)
    if process.poll() is not None:
        log_stream.close()
        raise RuntimeError(f"{engine} exited during startup; see {log_path}")
    return process, log_stream


def dnsperf_command(
    dnsperf: str,
    scenario: Scenario,
    host: str,
    port: int,
    query_file: Path,
    seconds: int,
    level: int,
    args: argparse.Namespace,
) -> list[str]:
    clients = min(level, args.max_clients)
    threads = min(clients, args.threads)
    return [
        dnsperf, "-j", "-m", scenario.mode, "-s", host, "-p", str(port), "-d", str(query_file),
        "-l", str(seconds), "-c", str(clients), "-T", str(threads), "-q", str(level), "-n", "1000000",
        "-t", str(args.timeout), "-O", "latency-histogram", "-O", "suppress=timeout,unexpected",
    ]


def run_dnsperf(command: list[str], output: Path) -> dict[str, Any]:
    completed = subprocess.run(command, cwd=BASE_DIR, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
    output.write_text(completed.stdout, encoding="utf-8")
    if completed.returncode != 0:
        raise RuntimeError(f"dnsperf failed ({completed.returncode}); see {output}")
    objects: list[dict[str, Any]] = []
    for line in completed.stdout.splitlines():
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(item, dict):
            objects.append(item)
    stats_objects = [item["statistics"] for item in objects if isinstance(item.get("statistics"), dict)]
    starts = [item["start"] for item in objects if isinstance(item.get("start"), dict)]
    if not stats_objects:
        raise RuntimeError(f"dnsperf JSON statistics missing; see {output}")
    stats = next((item for item in reversed(stats_objects) if not item.get("interval")), stats_objects[-1])
    latency = stats.get("latency", {})
    histogram = latency.get("histogram", [])
    return {
        "qps": float(stats.get("qps", 0.0)),
        "sent": int(stats.get("sent", 0)),
        "completed": int(stats.get("completed", 0)),
        "lost": int(stats.get("lost", 0)),
        "loss_pct": (float(stats.get("lost", 0)) / max(1.0, float(stats.get("sent", 0)))) * 100.0,
        "avg_latency_ms": float(latency.get("avg", 0.0)) * 1000.0,
        "p50_latency_ms": percentile_from_histogram(histogram, 0.50) or 0.0,
        "p95_latency_ms": percentile_from_histogram(histogram, 0.95) or 0.0,
        "p99_latency_ms": percentile_from_histogram(histogram, 0.99) or 0.0,
        "max_latency_ms": float(latency.get("max", 0.0)) * 1000.0,
        "dnsperf_version": str(starts[-1].get("version", "n/a")) if starts else "n/a",
    }


METRICS = (
    "qps", "loss_pct", "avg_latency_ms", "p50_latency_ms", "p95_latency_ms", "p99_latency_ms",
    "max_latency_ms", "cpu_pct_median", "cpu_pct_p95", "rss_mib_median", "rss_mib_max", "threads_max",
    "upstream_queries",
)


def aggregate(raw: list[dict[str, Any]]) -> list[dict[str, Any]]:
    groups: dict[tuple[str, int, str], list[dict[str, Any]]] = {}
    for row in raw:
        groups.setdefault((row["scenario"], row["load"], row["engine"]), []).append(row)
    result: list[dict[str, Any]] = []
    for (scenario, load, engine), rows in groups.items():
        item: dict[str, Any] = {"scenario": scenario, "load": load, "engine": engine, "repeats": len(rows)}
        for metric in METRICS:
            item[metric] = median(float(row.get(metric, 0.0)) for row in rows)
        result.append(item)
    return sorted(result, key=lambda row: (row["scenario"], row["load"], row["engine"]))


def write_tsv(path: Path, rows: list[dict[str, Any]]) -> None:
    fields = ("scenario", "load", "engine", "repeats", *METRICS)
    with path.open("w", encoding="utf-8") as stream:
        stream.write("\t".join(fields) + "\n")
        for row in rows:
            stream.write("\t".join(str(row.get(field, "")) for field in fields) + "\n")


def svg_frame(title: str, width: int, height: int, content: str) -> str:
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" role="img" aria-label="{html.escape(title)}" viewBox="0 0 {width} {height}">'
        '<style>text{font-family:system-ui,sans-serif;fill:#334155}.title{font-size:18px;font-weight:600}.label{font-size:12px}'
        '.grid{stroke:#cbd5e1;stroke-width:1}.axis{stroke:#64748b;stroke-width:1.2}</style>'
        f'<rect width="100%" height="100%" rx="12" fill="#fff"/><text x="24" y="30" class="title">{html.escape(title)}</text>{content}</svg>'
    )


def bar_chart(path: Path, title: str, values: list[tuple[str, dict[str, float]]], unit: str) -> None:
    active_engines = [engine for engine in ENGINES if any(engine in engine_values for _, engine_values in values)]
    group_height = 18 * len(active_engines) + 10
    width, height = 920, max(360, 100 + len(values) * group_height)
    # Keep a dedicated value-label gutter inside the SVG viewBox.  Placing a
    # label at the end of a maximum-width bar clips the text even when the
    # surrounding page has enough room.
    left, right, top, bottom = 220, 150, 62, 38
    plot_width = width - left - right
    maximum = max((max(engine_values.values(), default=0.0) for _, engine_values in values), default=1.0) or 1.0
    parts = [f'<line x1="{left}" y1="{top}" x2="{left}" y2="{height-bottom}" class="axis"/>']
    for index, (label, engine_values) in enumerate(values):
        y = top + index * group_height
        parts.append(f'<text x="{left-10}" y="{y+14}" text-anchor="end" class="label">{html.escape(label)}</text>')
        for engine_index, engine in enumerate(active_engines):
            if engine not in engine_values:
                continue
            value = engine_values[engine]
            offset = engine_index * 18
            bar_width = value / maximum * plot_width
            parts.append(f'<rect x="{left}" y="{y+offset}" width="{bar_width:.2f}" height="14" rx="3" fill="{ENGINE_COLORS[engine]}"/>')
            parts.append(f'<text x="{min(width-4, left+bar_width+5):.2f}" y="{y+offset+11}" class="label">{value:,.1f}{unit}</text>')
    legend_x = left
    for engine in active_engines:
        parts.append(f'<rect x="{legend_x}" y="{height-24}" width="12" height="12" fill="{ENGINE_COLORS[engine]}"/><text x="{legend_x+18}" y="{height-14}" class="label">{ENGINE_LABELS[engine]}</text>')
        legend_x += 150
    path.write_text(svg_frame(title, width, height, "".join(parts)), encoding="utf-8")


def line_chart(path: Path, title: str, rows: list[dict[str, Any]], metric: str, unit: str) -> None:
    width, height = 920, 430
    # The legend gets its own row below the title so long scenario names do not
    # collide with the first legend item when the SVG is scaled down.
    left, right, top, bottom = 72, 30, 78, 64
    loads = sorted({int(row["load"]) for row in rows})
    maximum = max((float(row[metric]) for row in rows), default=1.0) or 1.0
    x_positions = {load: left + index * (width-left-right) / max(1, len(loads)-1) for index, load in enumerate(loads)}
    parts: list[str] = []
    for tick in range(6):
        value = maximum * tick / 5
        y = height - bottom - (height-top-bottom) * tick / 5
        parts.append(f'<line x1="{left}" y1="{y:.2f}" x2="{width-right}" y2="{y:.2f}" class="grid"/>')
        parts.append(f'<text x="{left-8}" y="{y+4:.2f}" text-anchor="end" class="label">{value:,.1f}</text>')
    for load in loads:
        x = x_positions[load]
        parts.append(f'<text x="{x:.2f}" y="{height-bottom+22}" text-anchor="middle" class="label">{load}</text>')
    active_engines = [engine for engine in ENGINES if any(row["engine"] == engine for row in rows)]
    for engine in active_engines:
        engine_rows = sorted((row for row in rows if row["engine"] == engine), key=lambda row: row["load"])
        points = []
        for row in engine_rows:
            x = x_positions[int(row["load"])]
            y = height-bottom-float(row[metric])/maximum*(height-top-bottom)
            points.append(f"{x:.2f},{y:.2f}")
            parts.append(f'<circle cx="{x:.2f}" cy="{y:.2f}" r="4" fill="{ENGINE_COLORS[engine]}"/>')
        parts.append(f'<polyline points="{" ".join(points)}" fill="none" stroke="{ENGINE_COLORS[engine]}" stroke-width="3"/>')
    parts.append(f'<text x="{(left+width-right)/2}" y="{height-12}" text-anchor="middle" class="label">Outstanding queries</text>')
    parts.append(f'<text x="18" y="{(top+height-bottom)/2}" transform="rotate(-90 18 {(top+height-bottom)/2})" text-anchor="middle" class="label">{html.escape(unit)}</text>')
    legend_x = left
    for engine in active_engines:
        parts.append(f'<rect x="{legend_x}" y="44" width="12" height="12" fill="{ENGINE_COLORS[engine]}"/><text x="{legend_x+18}" y="55" class="label">{ENGINE_LABELS[engine]}</text>')
        legend_x += 135
    path.write_text(svg_frame(title, width, height, "".join(parts)), encoding="utf-8")


def best_rows(summary: list[dict[str, Any]]) -> list[dict[str, Any]]:
    groups: dict[tuple[str, str], list[dict[str, Any]]] = {}
    for row in summary:
        if float(row["loss_pct"]) <= 0.1:
            groups.setdefault((row["scenario"], row["engine"]), []).append(row)
    return [max(rows, key=lambda row: float(row["qps"])) for rows in groups.values()]


def chart_values(rows: list[dict[str, Any]], metric: str) -> list[tuple[str, dict[str, float]]]:
    by_scenario: dict[str, dict[str, float]] = {}
    for row in rows:
        by_scenario.setdefault(row["scenario"], {})[row["engine"]] = float(row[metric])
    return sorted(by_scenario.items())


def domain_matching_chart(path: Path, summary: list[dict[str, Any]]) -> None:
    selected = {(row["scenario"], row["engine"]): row for row in best_rows(summary)}
    domain_rows = {
        engine: selected.get(("51-common-domain-set", engine)) for engine in ENGINES
    }
    local_rows = {
        engine: selected.get(("50-common-local-answers", engine)) for engine in ENGINES
    }
    if any(domain_rows[engine] is None or local_rows[engine] is None for engine in ENGINES):
        return

    width, height = 920, 350
    left, plot_width, top = 150, 455, 82
    maximum = max(float(domain_rows[engine]["qps"]) for engine in ENGINES)  # type: ignore[index]
    parts = [
        '<text x="24" y="54" class="label">Stable QPS; retained capacity = domain-set QPS / local-answer QPS (higher is better)</text>'
    ]
    for index, engine in enumerate(ENGINES):
        domain_row = domain_rows[engine]
        local_row = local_rows[engine]
        assert domain_row is not None and local_row is not None
        qps = float(domain_row["qps"])
        retained = qps / float(local_row["qps"]) * 100.0
        y = top + index * 56
        bar_width = qps / maximum * plot_width
        parts.extend([
            f'<text x="{left-12}" y="{y+17}" text-anchor="end" class="label">{ENGINE_LABELS[engine]}</text>',
            f'<rect x="{left}" y="{y}" width="{bar_width:.2f}" height="22" rx="4" fill="{ENGINE_COLORS[engine]}"/>',
            f'<text x="{left+bar_width+7:.2f}" y="{y+17}" class="label">{qps:,.1f} QPS</text>',
            f'<text x="735" y="{y+17}" class="label">{retained:.1f}% retained</text>',
        ])
    parts.append(
        '<text x="24" y="332" class="label">Corpus: 143,366 normalized domains; real positive-match DNS response asserted before timing</text>'
    )
    path.write_text(
        svg_frame("143,366-domain positive-match throughput", width, height, "".join(parts)),
        encoding="utf-8",
    )


def generate_assets(result_dir: Path, summary: list[dict[str, Any]]) -> None:
    charts = result_dir / "charts"
    charts.mkdir(exist_ok=True)
    best = best_rows(summary)
    bar_chart(charts / "throughput.svg", "Maximum stable throughput (loss ≤ 0.1%)", chart_values(best, "qps"), " QPS")
    bar_chart(
        charts / "stable-tail-latency.svg",
        "p99 latency at maximum stable throughput",
        chart_values(best, "p99_latency_ms"),
        " ms",
    )
    bar_chart(charts / "cpu.svg", "Server CPU at maximum stable throughput", chart_values(best, "cpu_pct_median"), "%")
    bar_chart(charts / "memory.svg", "Server RSS at maximum stable throughput", chart_values(best, "rss_mib_median"), " MiB")
    domain_matching_chart(charts / "domain-matching.svg", summary)
    focus = "47-server-local-udp"
    focus_rows = [row for row in summary if row["scenario"] == focus]
    if not focus_rows:
        focus = summary[0]["scenario"]
        focus_rows = [row for row in summary if row["scenario"] == focus]
    line_chart(charts / "scaling.svg", f"Throughput scaling — {focus}", focus_rows, "qps", "QPS")
    line_chart(charts / "tail-latency.svg", f"p99 latency under load — {focus}", focus_rows, "p99_latency_ms", "ms")


def stable_table_lines(summary: list[dict[str, Any]], language: str) -> list[str]:
    header = "| 场景 | 引擎 | 并发 | QPS | p99 | CPU | RSS | 丢包 |" if language == "zh" else "| Scenario | Engine | Outstanding | QPS | p99 | CPU | RSS | Loss |"
    lines = [header, "|---|---|---:|---:|---:|---:|---:|---:|"]
    for row in sorted(best_rows(summary), key=lambda item: (item["scenario"], ENGINES.index(item["engine"]))):
        lines.append(
            f"| {row['scenario']} | {ENGINE_LABELS[row['engine']]} | {int(row['load'])} | {float(row['qps']):,.1f} | "
            f"{float(row['p99_latency_ms']):.3f} ms | {float(row['cpu_pct_median']):.1f}% | "
            f"{float(row['rss_mib_median']):.1f} MiB | {float(row['loss_pct']):.4f}% |"
        )
    return lines


def render_report(result_dir: Path, environment: dict[str, Any], summary: list[dict[str, Any]], args: argparse.Namespace) -> str:
    scenarios = {str(row["scenario"]) for row in summary}
    engines = {str(row["engine"]) for row in summary}
    common_matrix = set(DEFAULT_SCENARIOS).issubset(scenarios) and engines == set(ENGINES)
    native_specialized = scenarios == set(NATIVE_SPECIALIZED_SCENARIOS) and engines == {"oxidns", "mosdns"}
    report_title = "OxiDNS vs mosdns native-path benchmark report" if native_specialized else "Multi-engine DNS publishable benchmark report"
    lines = [
        f"# {report_title}", "",
        f"Generated: `{environment['timestamp']}`", "",
        "## Method", "",
        f"- repeats per point: `{args.repeats}`; measured duration: `{args.seconds}s`; warmup: `{args.warmup_seconds}s`",
        f"- outstanding-query levels: `{args.load_levels}`",
        "- aggregation: median across repeats; stable capacity excludes points with loss above 0.1%",
        "- server CPU is aggregate process CPU (100% = one fully occupied logical CPU); memory is sampled RSS",
        "- engines run one at a time and their order alternates by repeat/load point",
        "", "## Environment", "",
    ]
    lines.extend(f"- `{key}={value}`" for key, value in environment.items())
    lines.extend(["", "## Semantic equivalence checks", ""])
    if common_matrix:
        lines.extend([
            "- The common domain corpus is generated once and shared by all four engines. Supported `full:` prefixes are stripped; plain and `domain:` entries are retained; regex/keyword rules, invalid names, and duplicates are excluded because they cannot be mapped safely to every product.",
            f"- Normalized corpus statistics: `{environment.get('common_domain_corpus', 'n/a')}`.",
            f"- Before timing, every engine must return `{COMMON_DOMAIN_HIT_IP}` for a corpus hit and `{COMMON_DOMAIN_MISS_IP}` for the fixed miss control. The parsed DNS response evidence is saved in `semantic-validation.json`.",
            "- Positive and negative caches are filled by the deterministic local authority. Every timed cache row must record zero upstream queries or publication is rejected.",
            "- Response-IP/CIDR matching is excluded: the products expose different in-memory matching, response-filtering, and operating-system ipset semantics, so converting the input file would not create an equivalent workload.",
        ])
    elif native_specialized:
        probes_per_engine = sum(
            len(SEMANTIC_PROBES.get(scenario, ()))
            + len(SEMANTIC_AAAA_PROBES.get(scenario, ()))
            + len(SEMANTIC_RCODE_PROBES.get(scenario, ()))
            for scenario in NATIVE_SPECIALIZED_SCENARIOS
        )
        lines.extend([
            "- OxiDNS and mosdns load the same native domain-set files, including plain, `full:`, and `regexp:` rules. The timed workload mixes ten hits with eight misses.",
            "- The same CIDR response-IP set, local redirect/answer/TTL chain, and combined provider chain are exercised in both engines.",
            f"- Each engine must pass {probes_per_engine} exact A/AAAA/RCODE assertions ({probes_per_engine * 2} total), including explicit `full:`, plain suffix, `regexp:`, CIDR hit/miss, and rewritten TTL probes. Packet-level evidence is saved in `semantic-validation.json`.",
            "- This suite is intentionally separate from the four-engine normalized matrix; these native paths are not claimed to be equivalent to AdGuard Home or SmartDNS.",
        ])
    else:
        lines.append("- Scenario-specific A/RCODE assertions must pass before timing; evidence is saved in `semantic-validation.json`.")
    lines.extend([
        "", "## How to read the metrics", "",
        "- **QPS / throughput: higher is better**, provided loss and tail latency remain acceptable.",
        "- **p50/p95/p99/max latency: lower is better**. p99 is the response time that 99% of completed requests do not exceed; it is more useful than the average for spotting queueing and long-tail stalls.",
        "- **Packet loss: lower is better**. This report only treats a point as stable when median loss is at most 0.1%.",
        "- **CPU: lower is better at the same throughput**. CPU alone is not a speed score: higher CPU can be reasonable when it produces substantially more QPS. Here, 100% means one fully occupied logical CPU.",
        "- **RSS memory: lower is better for the same workload**. RSS is the process's resident physical memory during the measured run.",
        "- On scaling charts, the preferred curve rises with concurrency while latency and loss stay controlled. A flat QPS curve combined with rising p99 means the engine has reached saturation.",
    ])
    if common_matrix:
        lines.extend([
            "", "## Part I: Four-engine common matrix", "",
            "This section compares only paths that all four products can express with equivalent input and response semantics. The separate OxiDNS/mosdns native-rule suite is preserved as Part II and is not merged into this ranking.", "",
            "### Shared positive domain-match workload", "", "![Domain matching](charts/domain-matching.svg)",
        ])
    charts_heading = "### Charts" if common_matrix else "## Charts"
    stable_heading = "### Maximum stable points in the four-engine matrix" if common_matrix else "## Maximum stable point by scenario"
    lines.extend([
        "", charts_heading, "", "![Throughput](charts/throughput.svg)", "", "![p99 at maximum stable throughput](charts/stable-tail-latency.svg)", "", "![Scaling](charts/scaling.svg)", "", "![Tail latency](charts/tail-latency.svg)", "", "![CPU](charts/cpu.svg)", "", "![Memory](charts/memory.svg)", "", stable_heading, "",
    ])
    lines.extend(stable_table_lines(summary, "en"))
    lines.extend(["", "## Representativeness assessment", ""])
    if native_specialized:
        lines.extend([
            "This suite represents the two engines' native domain and response-IP rules, local policy chain, and combined provider/matcher pipeline under equivalent configuration and response semantics. It complements, but must not be merged with, the four-engine common matrix.", "",
            "It does not represent cold loading/reload time, upstream forwarding, encrypted transports, cross-host traffic, or products without the same native paths.", "",
        ])
    else:
        lines.extend([
            "This matrix represents stable local UDP and TCP request paths that all four products can configure with equivalent observable semantics: listener overhead, A/AAAA local answers, positive and negative warm-cache hits, and normalized domain lookup. The load sweep exposes scaling, saturation, and queueing instead of reducing the comparison to one peak-QPS number.", "",
            "The deterministic cache-fill authority must receive zero requests during timed cache intervals, so public network and upstream-server capacity are excluded from those results.", "",
            "It does not represent cold start or reload cost, cache-miss-heavy forwarding, DoT/DoH/DoQ, public upstream quality, multi-machine network effects, DNSSEC validation, or host-integrated side effects such as ipset/nftset. Those need dedicated matrices and, for capacity claims, a separate load-generator host.", "",
        ])
    lines.extend([
        "## Interpretation limits", "",
        "This is a same-host loopback comparison. It is representative of local request-path cost and concurrency scaling on the recorded machine, not public-upstream quality or production capacity on other hardware.", "",
    ])
    return "\n".join(lines)


def docs_chart_panel(src: str, alt: str, heading: str, body: str) -> str:
    return "\n".join([
        '<div className="row benchmark-chart-panel">',
        '  <div className="col col--8">',
        f'    <img src="{src}" alt="{alt}" />',
        "  </div>",
        '  <div className="col col--4">',
        f"    <p><strong>{heading}</strong></p>",
        f"    <p>{body}</p>",
        "  </div>",
        "</div>",
    ])


def preserved_markdown_section(path: Path, name: str) -> str:
    if not path.is_file():
        return ""
    text = path.read_text(encoding="utf-8")
    start_marker = f"{{/* {name}:start */}}"
    end_marker = f"{{/* {name}:end */}}"
    start = text.find(start_marker)
    end = text.find(end_marker)
    if start < 0 or end < start:
        return ""
    return text[start : end + len(end_marker)]


def render_zh_docs(environment: dict[str, Any], summary: list[dict[str, Any]], args: argparse.Namespace | None) -> str:
    parameters = "测试参数见[完整原始报告](/benchmarks/staged/report.txt)"
    if args is not None:
        parameters = f"每个点重复 {args.repeats} 次，预热 {args.warmup_seconds} 秒、测量 {args.seconds} 秒，并发点为 {args.load_levels}"
    lines = [
        "---", "title: 性能测试", "sidebar_position: 8", "---", "", "# 性能测试", "",
        f"本页展示 OxiDNS `{environment['oxidns_version']}`、mosdns `{environment['mosdns_version']}`、AdGuard Home `{environment.get('adguardhome_version', 'n/a')}` 与 SmartDNS `{environment.get('smartdns_version', 'n/a')}` 的阶段性实测快照，dnsperf 版本为 `{environment['dnsperf_version']}`。仅在架构、关键请求路径、测试口径或重要里程碑发生明显变化时更新，不要求每个版本重复测试。", "",
        f"本轮数据采集于 `{environment['timestamp']}`。{parameters}。每个指标取多次重复的中位数；最大稳定吞吐只接受丢包率不高于 0.1% 的点。进程 CPU 的 100% 表示占满一个逻辑核。", "",
        "## 被测环境", "",
        f"* CPU：`{environment['cpu']}`，逻辑核 `{environment['logical_cpus']}`",
        f"* 内存：`{environment['memory']}`",
        f"* OxiDNS：`{environment['oxidns_version']}`，SHA-256 `{environment['oxidns_sha256']}`",
        f"* mosdns：`{environment['mosdns_version']}`，SHA-256 `{environment['mosdns_sha256']}`",
        f"* AdGuard Home：`{environment.get('adguardhome_version', 'n/a')}`，SHA-256 `{environment.get('adguardhome_sha256', 'n/a')}`",
        f"* SmartDNS：`{environment.get('smartdns_version', 'n/a')}`，SHA-256 `{environment.get('smartdns_sha256', 'n/a')}`",
        f"* dnsperf：`{environment['dnsperf_version']}`", "",
        "## 规则格式与响应语义核验", "",
        "* 域名集合只生成一次并由四款软件共同加载：去掉可安全映射的 `full:` 前缀，保留纯域名与 `domain:` 项，排除无法在四方保持等价的正则/关键字规则、无效名称和重复项。",
        f"* 规范化统计：`{environment.get('common_domain_corpus', 'n/a')}`。",
        f"* 计时前解析真实 DNS 响应：集合命中必须返回 `{COMMON_DOMAIN_HIT_IP}`，固定未命中控制必须返回 `{COMMON_DOMAIN_MISS_IP}`；证据保存在[语义断言](/benchmarks/staged/semantic-validation.json)。",
        "* 正缓存与负缓存由本地确定性上游预热；每个计时区间的上游请求计数必须为 0，否则运行直接失败，不发布该结果。",
        "* 响应 IP/CIDR 匹配不进入四引擎主矩阵：四款产品对应内存匹配、响应过滤或操作系统 ipset 副作用，单纯转换文本格式不能建立等价工作负载。", "",
        "## 指标怎么看", "",
        "* **QPS / 吞吐量：越高越好**，但前提是丢包率和尾延迟仍在可接受范围内。",
        "* **p50、p95、p99、最大延迟：越低越好**。p99 表示 99% 已完成请求的响应时间不超过该值，比平均值更容易看出排队和长尾卡顿。",
        "* **丢包率：越低越好**。本报告只有在丢包率中位数不超过 0.1% 时，才把该并发点计为“稳定”。",
        "* **CPU：相同吞吐量下越低越好**。不能脱离 QPS 单看 CPU；如果使用更多 CPU 换来了明显更高吞吐，仍可能是合理结果。这里 100% 表示占满一个逻辑核。",
        "* **RSS 内存：相同负载下越低越好**，表示测试过程中进程实际驻留在物理内存中的容量。",
        "* 看折线图时，理想状态是并发增加后 QPS 继续上升，同时 p99 和丢包保持稳定；如果 QPS 已经走平而 p99 快速升高，说明服务已经进入饱和区。", "",
        "## 一、四引擎通用性能矩阵", "",
        "这一部分只比较四款软件都能保持相同输入和响应语义的路径。域名集合场景使用规范化纯域名并计时正命中；OxiDNS/mosdns 的原生规则专项作为独立区块保留，不与四引擎排名混算。", "",
        "### 通用域名正命中", "",
        docs_chart_panel(
            "/img/benchmarks/staged/domain-matching.svg", "143,366 个域名的真实命中吞吐与容量保留率", "QPS 与容量保留率越高越好",
            "柱长是域名正命中的最大稳定 QPS；右侧百分比是它相对同引擎本地回答基线所保留的容量。具体优劣必须结合本轮完整曲线、重复波动、p99、CPU 和 RSS 后人工评述。",
        ), "",
        "### 吞吐与并发扩展", "",
        docs_chart_panel(
            "/img/benchmarks/staged/throughput.svg", "各场景最大稳定吞吐量柱状图", "越高越好",
            "柱子越高表示稳定状态下每秒完成的 DNS 请求越多。仍需结合 p99 和丢包判断，不能把高丢包下的峰值当作有效容量。",
        ), "",
        docs_chart_panel(
            "/img/benchmarks/staged/stable-tail-latency.svg", "各引擎最大稳定吞吐点的 p99", "在标注吞吐量下越低越好",
            "这张图把尾延迟放回各自的最大稳定容量点。必须和 QPS 一起看：在明显更低吞吐下取得更低 p99，并不自动代表容量更好。",
        ), "",
        docs_chart_panel(
            "/img/benchmarks/staged/scaling.svg", "并发扩展折线图", "上升且不过早走平更好",
            "并发增加时 QPS 应继续上升。曲线走平表示接近吞吐上限；此时若 p99 同时快速升高，说明已经进入排队和饱和区。",
        ), "",
        "### 尾延迟", "",
        docs_chart_panel(
            "/img/benchmarks/staged/tail-latency.svg", "p99 尾延迟折线图", "越低越好",
            "p99 越低，绝大多数请求的最慢部分越可控。随着并发增加仍保持平缓的曲线，比只看平均延迟更可靠。",
        ), "",
        "### CPU 与内存", "",
        docs_chart_panel(
            "/img/benchmarks/staged/cpu.svg", "CPU 占用柱状图", "相同吞吐量下越低越好",
            "100% 等于占满一个逻辑核。CPU 必须和 QPS 配合看：CPU 更高但吞吐提升更大并不一定更差，也可进一步比较每万 QPS 的 CPU 成本。",
        ), "",
        docs_chart_panel(
            "/img/benchmarks/staged/memory.svg", "RSS 内存柱状图", "相同负载下越低越好",
            "RSS 表示进程实际驻留的物理内存。柱子越低，常驻内存压力越小；比较时应确保场景、规则数据和负载一致。",
        ), "",
        "### 四引擎各场景最大稳定点", "",
    ]
    lines.extend(stable_table_lines(summary, "zh"))
    lines.extend([
        "", "## 代表性判断", "",
        "本矩阵对**四款软件都能保持相同可观察语义的稳定本地 UDP/TCP 请求路径**具有代表性：最小监听器、本地回答、正/负热缓存和真实域名集合查询被分开测试，并通过多档并发展示扩展、饱和与排队，而不是只比较一个峰值 QPS。", "",
        "它不能代表冷启动/热重载、以缓存未命中为主的转发、DoT/DoH/DoQ、公网上游质量、跨机网络开销、DNSSEC 验证，或 ipset/nftset 等宿主机副作用。响应 IP/CIDR 匹配也未放入四引擎主矩阵，因为四款产品对应的是内存响应匹配、响应过滤或操作系统 ipset 副作用等不同语义，不能仅靠转换文件格式就宣称等价。生产容量测试还应使用独立压测机，并为这些路径建立单独矩阵。", "",
        "## 口径限制", "",
        "本轮是同机 loopback 对比，适合观察本地请求路径成本、并发扩展和排队，不代表其他硬件上的生产容量，也不把公网转发上游波动混进默认结论。可下载：[完整报告](/benchmarks/staged/report.txt)、[聚合 TSV](/benchmarks/staged/summary.tsv)、[逐轮 JSON](/benchmarks/staged/summary.raw.json)、[语义断言](/benchmarks/staged/semantic-validation.json)、[环境快照](/benchmarks/staged/environment.json)。", "",
    ])
    return "\n".join(lines)


def declared_probe_count(scenario: str) -> int:
    count = (
        len(SEMANTIC_PROBES.get(scenario, ()))
        + len(SEMANTIC_AAAA_PROBES.get(scenario, ()))
        + len(SEMANTIC_RCODE_PROBES.get(scenario, ()))
    )
    return count * 2 if scenario in CACHE_FIXTURE_SCENARIOS else count


def validate_common_publication(result_dir: Path, summary: list[dict[str, Any]]) -> None:
    actual_scenarios = {str(row["scenario"]) for row in summary}
    actual_engines = {str(row["engine"]) for row in summary}
    if actual_scenarios != set(DEFAULT_SCENARIOS) or actual_engines != set(ENGINES):
        raise SystemExit(
            "four-engine publishing requires exactly "
            f"{', '.join(DEFAULT_SCENARIOS)} for {', '.join(ENGINES)}"
        )
    if any(int(row["repeats"]) < 3 for row in summary):
        raise SystemExit("four-engine publishing requires at least three repeats per point")
    if {int(row["load"]) for row in summary} != {1, 4, 16, 64, 256, 1024}:
        raise SystemExit("four-engine publishing requires load levels 1,4,16,64,256,1024")

    semantic_path = result_dir / "semantic-validation.json"
    if not semantic_path.is_file():
        raise SystemExit("four-engine publishing requires semantic-validation.json")
    validation_rows = json.loads(semantic_path.read_text(encoding="utf-8"))
    expected_keys = {
        (scenario, engine)
        for scenario in DEFAULT_SCENARIOS
        for engine in ENGINES
    }
    actual_keys = {
        (str(row.get("scenario")), str(row.get("engine")))
        for row in validation_rows
    }
    if actual_keys != expected_keys:
        raise SystemExit("four-engine semantic validation is incomplete")
    if any(
        len(row.get("probes", ())) != declared_probe_count(str(row.get("scenario")))
        for row in validation_rows
    ):
        raise SystemExit("four-engine semantic probe counts do not match the declared suite")

    raw_path = result_dir / "summary.raw.json"
    if not raw_path.is_file():
        raise SystemExit("four-engine publishing requires summary.raw.json")
    raw_rows = json.loads(raw_path.read_text(encoding="utf-8"))
    cache_rows = [row for row in raw_rows if str(row.get("scenario")) in CACHE_FIXTURE_SCENARIOS]
    if not cache_rows or any(int(row.get("upstream_queries", -1)) != 0 for row in cache_rows):
        raise SystemExit("timed warm-cache rows must prove zero deterministic-upstream queries")


def publish_docs(
    result_dir: Path,
    report: str,
    environment: dict[str, str],
    summary: list[dict[str, Any]],
    args: argparse.Namespace | None = None,
) -> None:
    validate_common_publication(result_dir, summary)
    generate_assets(result_dir, summary)
    asset_dir = REPO_DIR / "docs/static/img/benchmarks/staged"
    asset_dir.mkdir(parents=True, exist_ok=True)
    for source in (result_dir / "charts").glob("*.svg"):
        shutil.copy2(source, asset_dir / source.name)
    zh_path = REPO_DIR / "docs/docs/benchmarks.md"
    native_zh = preserved_markdown_section(zh_path, "native-specialized")
    rendered_zh = render_zh_docs(environment, summary, args)
    if native_zh:
        rendered_zh = rendered_zh.replace("\n## 代表性判断", f"\n{native_zh}\n\n## 代表性判断", 1)
    zh_path.write_text(rendered_zh, encoding="utf-8")
    english = report.replace("# Multi-engine DNS publishable benchmark report", "# Performance Benchmark", 1)
    english = english.replace("charts/", "/img/benchmarks/staged/")
    english_chart_panels = {
        "![Domain matching](/img/benchmarks/staged/domain-matching.svg)": docs_chart_panel(
            "/img/benchmarks/staged/domain-matching.svg", "Stable throughput and retained capacity with 143,366 domains", "Higher QPS and retained capacity are better",
            "Bar length is maximum stable positive-match QPS. The percentage is capacity retained against the same engine's local-answer baseline. Product conclusions require manual review of full curves, repeat spread, p99, CPU, and RSS.",
        ),
        "![Throughput](/img/benchmarks/staged/throughput.svg)": docs_chart_panel(
            "/img/benchmarks/staged/throughput.svg", "Maximum stable throughput by scenario", "Higher is better",
            "A taller bar means more DNS requests completed per second at a stable point. Check p99 and loss as well; a peak reached with excessive loss is not usable capacity.",
        ),
        "![Scaling](/img/benchmarks/staged/scaling.svg)": docs_chart_panel(
            "/img/benchmarks/staged/scaling.svg", "Throughput scaling by concurrency", "Rising without flattening early is better",
            "QPS should continue to rise as concurrency increases. A flat curve marks the throughput ceiling; if p99 rises at the same time, the engine is queueing and saturated.",
        ),
        "![p99 at maximum stable throughput](/img/benchmarks/staged/stable-tail-latency.svg)": docs_chart_panel(
            "/img/benchmarks/staged/stable-tail-latency.svg", "p99 at each engine's maximum stable point", "Lower is better at the stated throughput",
            "This puts tail latency beside each engine's selected stable-capacity point. Compare it with QPS because a lower p99 measured at much lower throughput is not automatically a better capacity result.",
        ),
        "![Tail latency](/img/benchmarks/staged/tail-latency.svg)": docs_chart_panel(
            "/img/benchmarks/staged/tail-latency.svg", "p99 tail latency under load", "Lower is better",
            "Lower p99 means the slowest part of normal traffic remains controlled. A curve that stays flat as concurrency grows is preferable to one that climbs sharply.",
        ),
        "![CPU](/img/benchmarks/staged/cpu.svg)": docs_chart_panel(
            "/img/benchmarks/staged/cpu.svg", "CPU at maximum stable throughput", "Lower is better at equal throughput",
            "100% is one fully occupied logical CPU. Read CPU together with QPS: using more CPU can be reasonable when it produces proportionally more throughput.",
        ),
        "![Memory](/img/benchmarks/staged/memory.svg)": docs_chart_panel(
            "/img/benchmarks/staged/memory.svg", "Resident memory at maximum stable throughput", "Lower is better for the same workload",
            "RSS is physical memory resident for the process. A shorter bar means lower memory pressure when scenario, rules, and load are equivalent.",
        ),
    }
    for chart_markdown, chart_panel in english_chart_panels.items():
        english = english.replace(chart_markdown, chart_panel)
    if "## How to read the metrics" not in english:
        english = english.replace(
            "## Charts",
            "## How to read the metrics\n\n"
            "- **QPS / throughput: higher is better**, provided loss and tail latency remain acceptable.\n"
            "- **p50/p95/p99/max latency: lower is better**. p99 exposes queueing and long-tail stalls better than an average.\n"
            "- **Packet loss: lower is better**. A point is stable here only when median loss is at most 0.1%.\n"
            "- **CPU: lower is better at the same throughput**. CPU alone is not a speed score; 100% means one fully occupied logical CPU.\n"
            "- **RSS memory: lower is better for the same workload**. It is the process's resident physical memory.\n"
            "- A flat QPS curve combined with rising p99 means the engine has reached saturation.\n\n"
            "## Charts",
            1,
        )
    if "## Representativeness assessment" not in english:
        english = english.replace(
            "## Interpretation limits",
            "## Representativeness assessment\n\n"
            "This matrix represents stable local UDP paths that all four products can configure equivalently: minimal listener overhead, local answers, warm-cache behavior, and dataset lookup. Its load sweep exposes scaling, saturation, and queueing instead of relying on one peak-QPS number.\n\n"
            "It does not cover cold start or reload cost, TCP/DoT/DoH/DoQ, cache-miss-heavy traffic, public upstream quality, multi-machine network effects, or host-integrated side effects such as ipset/nftset. Production-capacity work needs a separate load-generator host and dedicated matrices for those paths.\n\n"
            "## Interpretation limits",
            1,
        )
    stage_note = (
        f"This page presents a periodic benchmark snapshot of OxiDNS `{environment['oxidns_version']}`, "
        f"mosdns `{environment['mosdns_version']}`, AdGuard Home `{environment.get('adguardhome_version', 'n/a')}`, "
        f"and SmartDNS `{environment.get('smartdns_version', 'n/a')}`, measured with dnsperf `{environment['dnsperf_version']}`. "
        "It is updated for meaningful architecture, request-path, methodology, or milestone changes—not for every release.\n\n"
    )
    english = english.replace("Generated:", stage_note + "Generated:", 1)
    english_header = "---\ntitle: Performance Benchmark\nsidebar_position: 8\n---\n\n"
    english_path = REPO_DIR / "docs/i18n/en/docusaurus-plugin-content-docs/current/benchmarks.md"
    native_english = preserved_markdown_section(english_path, "native-specialized")
    if native_english:
        english = english.replace(
            "\n## Representativeness assessment",
            f"\n{native_english}\n\n## Representativeness assessment",
            1,
        )
    english_path.write_text(english_header + english, encoding="utf-8")
    raw_target = REPO_DIR / "docs/static/benchmarks/staged"
    raw_target.mkdir(parents=True, exist_ok=True)
    shutil.copy2(result_dir / "summary.tsv", raw_target / "summary.tsv")
    shutil.copy2(result_dir / "summary.raw.json", raw_target / "summary.raw.json")
    shutil.copy2(result_dir / "environment.json", raw_target / "environment.json")
    semantic_validation = result_dir / "semantic-validation.json"
    if semantic_validation.is_file():
        shutil.copy2(semantic_validation, raw_target / "semantic-validation.json")
    (raw_target / "report.txt").write_text(report, encoding="utf-8")
    print(f"published docs snapshot for {environment['oxidns_version']}")


def publish_native_specialized(
    result_dir: Path,
    report: str,
    environment: dict[str, str],
    summary: list[dict[str, Any]],
) -> None:
    expected_scenarios = set(NATIVE_SPECIALIZED_SCENARIOS)
    actual_scenarios = {str(row["scenario"]) for row in summary}
    actual_engines = {str(row["engine"]) for row in summary}
    if actual_scenarios != expected_scenarios or actual_engines != {"oxidns", "mosdns"}:
        raise SystemExit(
            "native-specialized publishing requires exactly "
            f"{', '.join(NATIVE_SPECIALIZED_SCENARIOS)} for oxidns and mosdns"
        )
    if any(int(row["repeats"]) < 3 for row in summary):
        raise SystemExit("native-specialized publishing requires at least three repeats per point")
    if {int(row["load"]) for row in summary} != {1, 4, 16, 64, 256, 1024}:
        raise SystemExit("native-specialized publishing requires load levels 1,4,16,64,256,1024")

    semantic_validation = result_dir / "semantic-validation.json"
    if not semantic_validation.is_file():
        raise SystemExit("native-specialized publishing requires semantic-validation.json")
    validation_rows = json.loads(semantic_validation.read_text(encoding="utf-8"))
    expected_validation_keys = {
        (scenario, engine)
        for scenario in NATIVE_SPECIALIZED_SCENARIOS
        for engine in ("oxidns", "mosdns")
    }
    actual_validation_keys = {
        (str(row.get("scenario")), str(row.get("engine")))
        for row in validation_rows
    }
    if actual_validation_keys != expected_validation_keys:
        raise SystemExit("native-specialized semantic validation is incomplete")
    expected_probe_counts = {
        scenario: declared_probe_count(scenario)
        for scenario in NATIVE_SPECIALIZED_SCENARIOS
    }
    if any(
        len(row.get("probes", ())) != expected_probe_counts.get(str(row.get("scenario")), -1)
        for row in validation_rows
    ):
        raise SystemExit("native-specialized semantic probe counts do not match the declared suite")

    # Recreate charts with the current renderer so publishing an older completed
    # result cannot retain stale dimensions or legends.
    generate_assets(result_dir, summary)
    asset_dir = REPO_DIR / "docs/static/img/benchmarks/staged"
    asset_dir.mkdir(parents=True, exist_ok=True)
    chart_names = {
        "throughput.svg": "native-specialized-throughput.svg",
        "stable-tail-latency.svg": "native-specialized-stable-tail-latency.svg",
        "scaling.svg": "native-specialized-domain-scaling.svg",
        "tail-latency.svg": "native-specialized-domain-tail-latency.svg",
        "cpu.svg": "native-specialized-cpu.svg",
        "memory.svg": "native-specialized-memory.svg",
    }
    for source_name, target_name in chart_names.items():
        shutil.copy2(result_dir / "charts" / source_name, asset_dir / target_name)

    method_match = re.search(
        r"repeats per point: `(\d+)`; measured duration: `(\d+)s`; warmup: `(\d+)s`",
        report,
    )
    levels_match = re.search(r"outstanding-query levels: `([^`]+)`", report)
    report_args = argparse.Namespace(
        repeats=int(method_match.group(1)) if method_match else max(int(row["repeats"]) for row in summary),
        seconds=int(method_match.group(2)) if method_match else 0,
        warmup_seconds=int(method_match.group(3)) if method_match else 0,
        load_levels=levels_match.group(1) if levels_match else ",".join(str(level) for level in sorted({int(row["load"]) for row in summary})),
    )
    specialized_report = render_report(result_dir, environment, summary, report_args)
    for source_name, target_name in chart_names.items():
        specialized_report = specialized_report.replace(
            f"charts/{source_name}",
            f"/img/benchmarks/staged/{target_name}",
        )
    specialized_report = specialized_report.replace(
        "`semantic-validation.json`",
        "[native-specialized-semantic-validation.json](/benchmarks/staged/native-specialized-semantic-validation.json)",
    )

    raw_target = REPO_DIR / "docs/static/benchmarks/staged"
    raw_target.mkdir(parents=True, exist_ok=True)
    shutil.copy2(result_dir / "summary.tsv", raw_target / "native-specialized-summary.tsv")
    shutil.copy2(result_dir / "summary.raw.json", raw_target / "native-specialized-summary.raw.json")
    shutil.copy2(result_dir / "environment.json", raw_target / "native-specialized-environment.json")
    shutil.copy2(semantic_validation, raw_target / "native-specialized-semantic-validation.json")
    (raw_target / "native-specialized-report.txt").write_text(specialized_report, encoding="utf-8")
    print(f"published native specialized snapshot for {environment['oxidns_version']}")


def main() -> int:
    args = parse_args()
    if args.publish_docs and args.publish_native_specialized:
        raise SystemExit("choose either --publish-docs or --publish-native-specialized")
    if args.publish_existing:
        result_dir = args.publish_existing.resolve()
        environment = json.loads((result_dir / "environment.json").read_text(encoding="utf-8"))
        raw = json.loads((result_dir / "summary.raw.json").read_text(encoding="utf-8"))
        report = (result_dir / "report.md").read_text(encoding="utf-8")
        summary = aggregate(raw)
        if args.publish_native_specialized:
            publish_native_specialized(result_dir, report, environment, summary)
        else:
            publish_docs(result_dir, report, environment, summary)
        return 0
    if platform.system() != "Linux" and not args.dry_run:
        raise SystemExit("publishable resource measurements require Linux /proc; use --dry-run elsewhere")
    for name in ("seconds", "warmup_seconds", "repeats", "threads", "max_clients", "timeout"):
        if getattr(args, name) < 1:
            raise SystemExit(f"--{name.replace('_', '-')} must be positive")
    levels = positive_levels(args.load_levels)
    engines_to_run = selected_engines(args.engines)
    selected = select_scenarios(load_catalog(BASE_DIR / "scenarios.tsv"), args.selectors)
    scenario_engines = {
        scenario.label: [engine for engine in engines_to_run if engine in scenario.engines]
        for scenario in selected
    }
    if not any(scenario_engines.values()):
        raise SystemExit("none of the selected engines support the selected scenarios")
    for scenario in selected:
        for path in (
            *(scenario.config_for(engine) for engine in scenario_engines[scenario.label]),
            scenario.query_file,
            scenario.warmup_file,
        ):
            if not path.is_file():
                raise SystemExit(f"missing benchmark input: {path}")
    print("selected scenarios:")
    for scenario in selected:
        active_labels = ",".join(scenario_engines[scenario.label]) or "skipped"
        print(f"  {scenario.label:30} [{active_labels}] {scenario.description}")
    scheduled_engines = [
        engine
        for engine in ENGINES
        if any(engine in active for active in scenario_engines.values())
    ]
    print(f"engines: {', '.join(scheduled_engines)}; load levels: {levels}; repeats: {args.repeats}")
    if args.dry_run:
        return 0

    corpus_stats: dict[str, int] | None = None
    common_scenario = next((scenario for scenario in selected if scenario.label == "51-common-domain-set"), None)
    if common_scenario is not None:
        corpus_stats = prepare_common_domain_assets(common_scenario.query_file)
        print(f"normalized common domain corpus: {corpus_stats}")

    binary_defaults = {
        "oxidns": str(BASE_DIR / "oxidns"),
        "mosdns": str(BASE_DIR / "mosdns"),
        "adguardhome": str(BASE_DIR / "AdGuardHome"),
        "smartdns": str(BASE_DIR / "smartdns"),
    }
    binaries = {
        engine: require_executable(os.getenv(f"{engine.upper()}_BIN_PATH", binary_defaults[engine]), ENGINE_LABELS[engine])
        for engine in scheduled_engines
    }
    dnsperf = require_executable(os.getenv("DNSPERF_BIN_PATH", "dnsperf"), "dnsperf")
    if "-j" not in command_output([dnsperf, "-h"]):
        raise SystemExit("dnsperf lacks JSON support (-j); run prepare_server.sh to build dnsperf 2.15.1")
    long_help = command_output([dnsperf, "-H"])
    if "latency-histogram" not in long_help:
        raise SystemExit("dnsperf lacks latency-histogram support")

    result_dir = args.result_dir or BASE_DIR / "results" / f"publishable-{datetime.now().strftime('%Y%m%d-%H%M%S')}"
    result_dir.mkdir(parents=True, exist_ok=False)
    environment = {
        "timestamp": datetime.now(timezone.utc).astimezone().isoformat(),
        "hostname": platform.node(),
        "kernel": platform.platform(),
        "cpu": command_output(["sh", "-c", "lscpu | grep 'Model name' | head -n1"]).replace("\n", " "),
        "logical_cpus": str(os.cpu_count() or 0),
        "memory": command_output(["sh", "-c", "free -h | grep '^Mem:'"]).replace("\n", " "),
        "git_head": git_value("rev-parse", "HEAD"),
        "git_describe": git_value("describe", "--tags", "--always", "--dirty"),
        "dnsperf_version": dnsperf_version(dnsperf),
    }
    environment["benchmark_inputs_sha256"] = benchmark_input_manifest(selected, scenario_engines)
    if corpus_stats is not None:
        environment["common_domain_corpus"] = json.dumps(corpus_stats, sort_keys=True)
    uses_fixture = any("fixture-upstream" in scenario.tags for scenario in selected)
    if uses_fixture:
        environment["cache_upstream_fixture"] = f"deterministic UDP authority at {FIXTURE_HOST}:{FIXTURE_PORT}"
    for engine, binary in binaries.items():
        environment[f"{engine}_version"] = binary_version(binary, engine)
        environment[f"{engine}_sha256"] = sha256(binary)
    (result_dir / "environment.json").write_text(json.dumps(environment, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    raw: list[dict[str, Any]] = []
    semantic_validation: list[dict[str, Any]] = []
    validated_semantics: set[tuple[str, str]] = set()
    active: subprocess.Popen[str] | None = None
    active_log: Any = None
    fixture: DnsFixture | None = None
    try:
        if uses_fixture:
            fixture = DnsFixture()
            fixture.start()
            print(f"deterministic cache-fill fixture: {FIXTURE_HOST}:{FIXTURE_PORT}")
        for scenario in selected:
            if not scenario_engines[scenario.label]:
                continue
            for level in levels:
                for repeat in range(1, args.repeats + 1):
                    engines = list(scenario_engines[scenario.label])
                    if (level + repeat + sum(scenario.label.encode())) % 2:
                        engines.reverse()
                    for engine in engines:
                        binary = binaries[engine]
                        config = scenario.config_for(engine)
                        host, port = extract_listen(config, engine, scenario.mode)
                        stem = f"{scenario.label}.{engine}.q{level}.r{repeat:02d}"
                        print(f">>> {stem}", flush=True)
                        active, active_log = start_engine(engine, binary, config, result_dir / f"{stem}.startup.log")
                        semantic_key = (scenario.label, engine)
                        if semantic_key not in validated_semantics:
                            validation = validate_semantics(scenario, engine, host, port)
                            semantic_validation.append(validation)
                            validated_semantics.add(semantic_key)
                            print(f"    semantic probes passed: {validation['probes']}")
                        warmup_command = dnsperf_command(dnsperf, scenario, host, port, scenario.warmup_file, args.warmup_seconds, level, args)
                        run_dnsperf(warmup_command, result_dir / f"{stem}.warmup.jsonl")
                        fixture_before = fixture.requests if fixture is not None and "fixture-upstream" in scenario.tags else 0
                        sampler = ProcSampler(active.pid, args.sample_interval, result_dir / f"{stem}.resources.tsv")
                        sampler.start()
                        measured_command = dnsperf_command(dnsperf, scenario, host, port, scenario.query_file, args.seconds, level, args)
                        measured = run_dnsperf(measured_command, result_dir / f"{stem}.dnsperf.jsonl")
                        measured.update(sampler.stop())
                        upstream_queries = (
                            fixture.requests - fixture_before
                            if fixture is not None and "fixture-upstream" in scenario.tags
                            else 0
                        )
                        measured["upstream_queries"] = upstream_queries
                        if upstream_queries:
                            raise RuntimeError(
                                f"{scenario.label}/{engine} sent {upstream_queries} upstream queries during the timed "
                                "warm-cache interval"
                            )
                        measured.update({"scenario": scenario.label, "engine": engine, "mode": scenario.mode, "load": level, "repeat": repeat})
                        raw.append(measured)
                        stop_process(active)
                        active = None
                        active_log.close()
                        active_log = None
                        if engine == "adguardhome":
                            shutil.rmtree(result_dir / f"{stem}.startup.work", ignore_errors=True)
                        print(f"    qps={measured['qps']:.1f} p99={measured['p99_latency_ms']:.3f}ms cpu={measured['cpu_pct_median']:.1f}% rss={measured['rss_mib_median']:.1f}MiB loss={measured['loss_pct']:.4f}%")
                        time.sleep(args.cooldown)
    finally:
        stop_process(active)
        if active_log is not None:
            active_log.close()
        if fixture is not None:
            fixture.stop()

    (result_dir / "summary.raw.json").write_text(json.dumps(raw, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    (result_dir / "semantic-validation.json").write_text(
        json.dumps(semantic_validation, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    if raw:
        environment["dnsperf_version"] = str(raw[0].get("dnsperf_version", environment["dnsperf_version"]))
        (result_dir / "environment.json").write_text(json.dumps(environment, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    summary = aggregate(raw)
    write_tsv(result_dir / "summary.tsv", summary)
    generate_assets(result_dir, summary)
    report = render_report(result_dir, environment, summary, args)
    (result_dir / "report.md").write_text(report, encoding="utf-8")
    if args.publish_docs:
        publish_docs(result_dir, report, environment, summary, args)
    if args.publish_native_specialized:
        publish_native_specialized(result_dir, report, environment, summary)
    print(f"report: {result_dir / 'report.md'}")
    return 0


if __name__ == "__main__":
    signal.signal(signal.SIGPIPE, signal.SIG_DFL)
    raise SystemExit(main())
