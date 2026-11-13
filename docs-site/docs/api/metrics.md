# Metrics API

Endpoints under `/api/v1/metrics`.

## `GET` /metrics

**What the server says about itself for a monitoring system (Prometheus text). Needs the scrape token; without one configured there is no such route.**

### Responses

| Status | Description |
|---|---|
| `200` | Request counts and times, and the runs waiting and running |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---
