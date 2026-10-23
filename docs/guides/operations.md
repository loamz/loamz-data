# Operations

## Topologies
- **Laptop or evaluation:** `deploy/compose` (`./gen-env.sh`, `docker compose --profile agents up -d`).
- **Production:** the `loamz-cell` Helm chart on Kubernetes, with managed PostgreSQL and object storage from the
  Terraform modules (`deploy/terraform/{aws,gcp,azure}`). Modes: `monolith` (one agent server) or `microservices`.
- A *cell* is one customer's complete installation, in the customer's own cloud account.

## Availability
- Stateless workloads (agent server, data API, workers, consoles) run several replicas; the chart adds a
  PodDisruptionBudget (`maxUnavailable: 1`) and a soft node-spread constraint to any component with two or more
  replicas, and an optional HorizontalPodAutoscaler.
- The data controller schedules jobs and compute from one active replica. By default its queue is on a persistent
  volume and it restarts in place. With `data.controller.ha.enabled=true` the queue lives in PostgreSQL (database
  `loamz_compute`), two or more replicas run, one is elected, and a standby takes over within seconds if the active one
  fails: it fails the jobs that were running (they retry within their attempt limits) and resumes scheduling. A standby
  answers `/readyz` with 503, so the Service routes only to the active one.
- PostgreSQL: use the Terraform `db_multi_az` option for a managed standby.

## Health and observability
- Probes: `/health` and `/health/ready` (agent server), `/readyz` (data API, controller, workers).
- **Metrics.** The agent server and the data API serve Prometheus metrics at `/metrics`:
  `loamz_http_requests_total` (method, route template, status class), `loamz_http_request_duration_seconds`
  (histogram) and `loamz_http_requests_in_flight`, labelled by `service`. Labels are route templates, so cardinality is
  bounded. Keep `/metrics` on the cluster network, and set `LOAMZ_METRICS_TOKEN` to also require a bearer token. In the
  chart, `metrics.enabled=true` lets the monitoring namespace through the network policy;
  `metrics.serviceMonitor.enabled` and `metrics.alerts.enabled` create a ServiceMonitor and a PrometheusRule (error
  rate, p95 latency, no reporting server) for the Prometheus Operator.
- Every request carries a W3C `traceparent`; the servers emit OpenTelemetry traces when `OTEL_*` is enabled, and
  structured logs. `deploy/observability` has Grafana, Loki, Promtail and Tempo configuration.
- Audit events stream to a SIEM through `audit-events:export`.

## Backup and restore
See [disaster-recovery.md](disaster-recovery.md): what to back up, the restore order, the recovery objectives and the
weekly restore drill.

## Upgrades
- Versions are tags: `cell-vX.Y.Z` images, and `loamz-*` packages released together by release-please.
- `helm upgrade` is rolling for stateless components. Run the cell's end-to-end script (`deploy/compose/e2e.py`
  adapted to your endpoint) after an upgrade as a smoke test.
- APIs are versioned under `/api/v1`; a breaking change ships as `/api/v2` beside v1.

## Capacity and limits
Per-tenant storage quotas, request rate limits and statement limits are enforced by the data service; compute is
sized per worker role (`sql`, `jobs`, `documents`) in the chart's `data.workers`.
