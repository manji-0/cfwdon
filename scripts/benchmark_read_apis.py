#!/usr/bin/env python3
"""Benchmark read-heavy Mastodon API routes with client latency stats.

Emits a unique User-Agent so Workers Observability can be correlated with
`event=api_request` logs (duration_ms) and D1 trace spans ($metadata.trigger).

Usage:
  python3 scripts/benchmark_read_apis.py --base-url https://fedi.manji.app
  python3 scripts/benchmark_read_apis.py --base-url https://fedi.manji.app --iterations 5
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass(frozen=True)
class Endpoint:
    name: str
    path: str
    session: bool  # wired to D1 Sessions (first-unconstrained on GET)
    notes: str = ""


def default_endpoints(status_id: str) -> list[Endpoint]:
    return [
        Endpoint("instance_v1", "/api/v1/instance", False, "metadata only"),
        Endpoint("instance_v2", "/api/v2/instance", False, "metadata only"),
        Endpoint(
            "instance_activity",
            "/api/v1/instance/activity",
            False,
            "aggregates; not session-wired",
        ),
        Endpoint("public_timeline", "/api/v1/timelines/public?limit=20", True),
        Endpoint(
            "public_timeline_local",
            "/api/v1/timelines/public?local=true&limit=20",
            True,
        ),
        Endpoint("custom_emojis", "/api/v1/custom_emojis", False),
        Endpoint("trends_tags", "/api/v1/trends/tags?limit=10", False),
        Endpoint("trends_statuses", "/api/v1/trends/statuses?limit=10", False),
        Endpoint(
            "account_lookup",
            "/api/v1/accounts/lookup?acct=manji0@fedi.manji.app",
            False,
        ),
        Endpoint("status_show", f"/api/v1/statuses/{status_id}", True),
        Endpoint("status_context", f"/api/v1/statuses/{status_id}/context", True),
        Endpoint("nodeinfo", "/.well-known/nodeinfo", False),
    ]


@dataclass
class Sample:
    status: int
    latency_ms: float
    bytes_read: int
    bookmark: str | None


def fetch(url: str, user_agent: str, bookmark: str | None) -> Sample:
    import subprocess

    headers = ["-H", f"User-Agent: {user_agent}", "-H", "Accept: application/json"]
    if bookmark:
        headers.extend(["-H", f"x-d1-bookmark: {bookmark}"])
    started = time.perf_counter()
    proc = subprocess.run(
        ["curl", "-sS", "-o", "/tmp/cfwdon_bench_body", "-w", "%{http_code}", *headers, url],
        capture_output=True,
        text=True,
        check=False,
    )
    latency_ms = (time.perf_counter() - started) * 1000
    status = int(proc.stdout.strip() or "0")
    try:
        body_size = Path("/tmp/cfwdon_bench_body").stat().st_size
    except OSError:
        body_size = 0
    bookmark_out = None
    header_proc = subprocess.run(
        ["curl", "-sSI", *headers, url],
        capture_output=True,
        text=True,
        check=False,
    )
    for line in header_proc.stdout.splitlines():
        if line.lower().startswith("x-d1-bookmark:"):
            bookmark_out = line.split(":", 1)[1].strip()
            break
    return Sample(status, latency_ms, body_size, bookmark_out)


def percentile(values: list[float], pct: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    index = max(0, min(len(ordered) - 1, int(round((pct / 100) * (len(ordered) - 1)))))
    return ordered[index]


def resolve_status_id(base_url: str, user_agent: str) -> str:
    import subprocess

    url = urllib.parse.urljoin(base_url, "/api/v1/timelines/public?local=true&limit=1")
    sample = fetch(url, user_agent, None)
    if sample.status != 200:
        raise RuntimeError(f"could not resolve status id: HTTP {sample.status}")
    proc = subprocess.run(
        ["curl", "-sS", "-H", f"User-Agent: {user_agent}", url],
        capture_output=True,
        text=True,
        check=True,
    )
    payload = json.loads(proc.stdout)
    if not payload:
        raise RuntimeError("public local timeline is empty; need a status id")
    return payload[0]["id"]


def benchmark_endpoint(
    base_url: str,
    endpoint: Endpoint,
    user_agent: str,
    warmup: int,
    iterations: int,
) -> dict[str, Any]:
    url = urllib.parse.urljoin(base_url, endpoint.path)
    bookmark: str | None = None

    for _ in range(warmup):
        sample = fetch(url, user_agent, bookmark)
        bookmark = sample.bookmark or bookmark
        if sample.status >= 500:
            raise RuntimeError(f"warmup failed for {endpoint.name}: HTTP {sample.status}")

    cold = fetch(url, user_agent, None)
    latencies: list[float] = []
    statuses: list[int] = []
    bookmarks_seen = 0
    for _ in range(iterations):
        sample = fetch(url, user_agent, bookmark)
        latencies.append(sample.latency_ms)
        statuses.append(sample.status)
        if sample.bookmark:
            bookmarks_seen += 1
            bookmark = sample.bookmark

    ok = [latency for latency, status in zip(latencies, statuses) if 200 <= status < 300]
    return {
        "name": endpoint.name,
        "path": endpoint.path,
        "session": endpoint.session,
        "notes": endpoint.notes,
        "cold_ms": round(cold.latency_ms, 1),
        "cold_status": cold.status,
        "warm_median_ms": round(statistics.median(ok), 1) if ok else None,
        "warm_p95_ms": round(percentile(ok, 95), 1) if ok else None,
        "warm_min_ms": round(min(ok), 1) if ok else None,
        "warm_max_ms": round(max(ok), 1) if ok else None,
        "statuses": statuses,
        "bookmark_return_rate": round(bookmarks_seen / max(iterations, 1), 2),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", default="https://fedi.manji.app")
    parser.add_argument("--warmup", type=int, default=1)
    parser.add_argument("--iterations", type=int, default=3)
    parser.add_argument("--run-id", default=uuid.uuid4().hex[:12])
    parser.add_argument("--json", action="store_true", help="print machine-readable report")
    args = parser.parse_args()

    user_agent = f"cfwdon-read-bench/{args.run_id}"
    status_id = resolve_status_id(args.base_url, user_agent)
    endpoints = default_endpoints(status_id)

    started_at = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    results = []
    for endpoint in endpoints:
        results.append(
            benchmark_endpoint(
                args.base_url,
                endpoint,
                user_agent,
                args.warmup,
                args.iterations,
            )
        )

    report = {
        "run_id": args.run_id,
        "user_agent": user_agent,
        "base_url": args.base_url,
        "started_at": started_at,
        "status_id": status_id,
        "warmup": args.warmup,
        "iterations": args.iterations,
        "endpoints": results,
        "observability_hint": {
            "api_request_filter": {
                "user_agent": user_agent,
                "event": "api_request",
            },
            "d1_filter": {
                "$metadata.trigger": "includes GET /api/",
                "cloudflare.binding.type": "d1",
            },
        },
    }

    if args.json:
        print(json.dumps(report, indent=2))
        return 0

    print(f"run_id={args.run_id} user_agent={user_agent}")
    print(f"started_at={started_at} status_id={status_id}")
    print()
    print(
        f"{'endpoint':<22} {'session':<7} {'cold':>8} {'p50':>8} {'p95':>8} {'status':<8} bookmark"
    )
    print("-" * 80)
    for row in results:
        status = ",".join(str(code) for code in row["statuses"])
        print(
            f"{row['name']:<22} "
            f"{'yes' if row['session'] else 'no':<7} "
            f"{row['cold_ms']:>7.0f}ms "
            f"{(row['warm_median_ms'] or 0):>7.0f}ms "
            f"{(row['warm_p95_ms'] or 0):>7.0f}ms "
            f"{status:<8} "
            f"{row['bookmark_return_rate']:.0%}"
        )
    print()
    print(
        "Correlate in Workers Observability: filter user_agent="
        f"{user_agent!r} and event=api_request for server duration_ms; "
        "filter cloudflare.binding.type=d1 grouped by $metadata.trigger for query counts."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
