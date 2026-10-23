"""Prometheus metrics for both servers: request count, latency and in-flight requests, at `/metrics`.

    from loamz.server.metrics import install
    install(app, service="data")

Labels are the HTTP method, the route *template* (`/api/v1/threads/{thread_id}`, never the concrete path) and the
status class, so cardinality stays bounded. The middleware is plain ASGI, so streamed responses (server-sent events)
are not buffered. `/metrics` is meant for the cluster network; set `LOAMZ_METRICS_TOKEN` to require
`Authorization: Bearer <token>` as well.
"""

from __future__ import annotations

import hmac
import os
import threading
import time
from collections import defaultdict
from typing import Any

from fastapi import FastAPI, Request, Response

BUCKETS = (0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0)
CONTENT_TYPE = "text/plain; version=0.0.4; charset=utf-8"


def _escape(value: str) -> str:
    return value.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")


class Registry:
    def __init__(self, service: str) -> None:
        self.service = service
        self.lock = threading.Lock()
        self.started = time.time()
        self.requests: dict[tuple[str, str, str], int] = defaultdict(int)
        self.buckets: dict[tuple[str, str], list[int]] = defaultdict(lambda: [0] * len(BUCKETS))
        self.sums: dict[tuple[str, str], float] = defaultdict(float)
        self.counts: dict[tuple[str, str], int] = defaultdict(int)
        self.in_flight = 0

    def begin(self) -> None:
        with self.lock:
            self.in_flight += 1

    def finish(self, method: str, route: str, status: int, seconds: float) -> None:
        with self.lock:
            self.in_flight -= 1
            self.requests[(method, route, f"{status // 100}xx")] += 1
            key = (method, route)
            self.sums[key] += seconds
            self.counts[key] += 1
            counts = self.buckets[key]
            for i, bound in enumerate(BUCKETS):
                if seconds <= bound:
                    counts[i] += 1

    def render(self) -> str:
        svc = _escape(self.service)
        lines = [
            "# HELP loamz_http_requests_total HTTP requests by method, route template and status class.",
            "# TYPE loamz_http_requests_total counter",
        ]
        with self.lock:
            for (method, route, status), n in sorted(self.requests.items()):
                labels = (
                    f'service="{svc}",method="{method}",route="{_escape(route)}",status="{status}"'
                )
                lines.append(f"loamz_http_requests_total{{{labels}}} {n}")
            lines += [
                "# HELP loamz_http_request_duration_seconds Time to produce the response.",
                "# TYPE loamz_http_request_duration_seconds histogram",
            ]
            for key in sorted(self.counts):
                method, route = key
                base = f'service="{svc}",method="{method}",route="{_escape(route)}"'
                for bound, n in zip(BUCKETS, self.buckets[key], strict=True):
                    lines.append(
                        f'loamz_http_request_duration_seconds_bucket{{{base},le="{bound}"}} {n}'
                    )
                lines.append(
                    f'loamz_http_request_duration_seconds_bucket{{{base},le="+Inf"}} {self.counts[key]}'
                )
                lines.append(f"loamz_http_request_duration_seconds_sum{{{base}}} {self.sums[key]}")
                lines.append(
                    f"loamz_http_request_duration_seconds_count{{{base}}} {self.counts[key]}"
                )
            lines += [
                "# HELP loamz_http_requests_in_flight Requests being handled now.",
                "# TYPE loamz_http_requests_in_flight gauge",
                f'loamz_http_requests_in_flight{{service="{svc}"}} {self.in_flight}',
                "# HELP loamz_process_start_time_seconds Start time of the process.",
                "# TYPE loamz_process_start_time_seconds gauge",
                f'loamz_process_start_time_seconds{{service="{svc}"}} {self.started}',
            ]
        return "\n".join(lines) + "\n"


class MetricsMiddleware:
    def __init__(self, app: Any, registry: Registry) -> None:
        self.app, self.registry = app, registry

    async def __call__(self, scope: dict, receive: Any, send: Any) -> None:
        if scope["type"] != "http" or scope["path"] == "/metrics":
            await self.app(scope, receive, send)
            return
        status = 500
        started = time.perf_counter()
        self.registry.begin()

        async def capture(message: dict) -> None:
            nonlocal status
            if message["type"] == "http.response.start":
                status = message["status"]
            await send(message)

        try:
            await self.app(scope, receive, capture)
        finally:
            route = scope.get("route")
            template = getattr(route, "path", None) or "unmatched"
            self.registry.finish(scope["method"], template, status, time.perf_counter() - started)


def install(app: FastAPI, *, service: str) -> Registry:
    registry = Registry(service)
    app.add_middleware(MetricsMiddleware, registry=registry)

    @app.get("/metrics", include_in_schema=False)
    def metrics(request: Request) -> Response:
        token = os.environ.get("LOAMZ_METRICS_TOKEN")
        if token:
            given = request.headers.get("authorization", "").removeprefix("Bearer ").strip()
            if not hmac.compare_digest(given, token):
                return Response(status_code=401, headers={"WWW-Authenticate": "Bearer"})
        return Response(registry.render(), media_type=CONTENT_TYPE)

    return registry
