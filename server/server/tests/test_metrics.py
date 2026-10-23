"""Prometheus metrics: route templates as labels, streaming untouched, optional bearer token."""

import asyncio

from fastapi import FastAPI
from fastapi.responses import StreamingResponse
from fastapi.testclient import TestClient

from loamz.server.metrics import install


def make() -> tuple[FastAPI, TestClient]:
    app = FastAPI()
    install(app, service="test")

    @app.get("/things/{thing_id}")
    def thing(thing_id: str) -> dict[str, str]:
        return {"id": thing_id}

    @app.get("/stream")
    async def stream() -> StreamingResponse:
        async def chunks():
            for i in range(3):
                await asyncio.sleep(0)
                yield f"data: {i}\n\n"

        return StreamingResponse(chunks(), media_type="text/event-stream")

    return app, TestClient(app)


def test_requests_are_counted_by_route_template_not_concrete_path():
    _, client = make()
    client.get("/things/a")
    client.get("/things/b")
    client.get("/nowhere")
    text = client.get("/metrics").text
    assert 'route="/things/{thing_id}",status="2xx"} 2' in text
    assert 'route="unmatched",status="4xx"} 1' in text
    assert "/things/a" not in text
    assert (
        'loamz_http_request_duration_seconds_count{service="test",method="GET",route="/things/{thing_id}"} 2'
        in text
    )
    assert 'le="+Inf"} 2' in text and "loamz_http_requests_in_flight" in text


def test_the_metrics_endpoint_does_not_count_itself_and_streams_pass_through():
    _, client = make()
    client.get("/metrics")
    body = client.get("/stream").text
    assert body == "data: 0\n\ndata: 1\n\ndata: 2\n\n"
    text = client.get("/metrics").text
    assert 'route="/metrics"' not in text and 'route="/stream",status="2xx"} 1' in text


def test_a_token_is_required_when_configured(monkeypatch):
    monkeypatch.setenv("LOAMZ_METRICS_TOKEN", "s3cret")
    _, client = make()
    assert client.get("/metrics").status_code == 401
    assert client.get("/metrics", headers={"Authorization": "Bearer wrong"}).status_code == 401
    assert client.get("/metrics", headers={"Authorization": "Bearer s3cret"}).status_code == 200
