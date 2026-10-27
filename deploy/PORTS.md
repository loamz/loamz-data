# Port registry

One cell, one table: no two components share a port, in compose, in Kubernetes, or when run side by side on a
developer machine. Change a number here first, then in `deploy/compose/.env.example`, the chart's `values.yaml`
and the Dockerfile `EXPOSE`/entrypoint defaults.

"Published" means bound on the Docker host (always on `BIND_ADDR`, default `127.0.0.1`). Everything else is reachable only
on the cell's private network.

| Port | Component | Compose | Notes |
| --- | --- | --- | --- |
| 3011 | data-workspace (data workspace UI) | published (`PORT_DATA_WEB`) | |
| 5432 | PostgreSQL | internal | TLS on; databases `keycloak`, `loamz_data`, `data_retrieval`, `loamz_compute` |
| 8020 | loamz-data control plane API | published (`PORT_DATA_API`) | `/api/v1`, `/readyz` |
| 8031 | llama-embed (profile `ml`) | internal | llama.cpp embedding sidecar |
| 8032 | llama-rerank (profile `ml`) | internal | llama.cpp reranker sidecar |
| 8040 | loamz-data controller | internal | |
| 8041 | loamz-data worker, `sql` | internal | |
| 8042 | loamz-data worker, `jobs` | internal | |
| 8043 | loamz-data worker, `documents` | internal | |
| 8044 | loamz-data worker, `inference` | reserved | not deployed by default |
| 8050 | loamz-data worker `inference` (profile `ml`) | internal | image default is 8080 |
| 8060 | loamz-reader | reserved | image default is 8080; start with `--port 8060` |
| 8081 | Keycloak | published (`PORT_KEYCLOAK`) | management/health on 9000 (internal) |
| 8181 | SeaweedFS Iceberg REST catalog | internal | |
| 8333 | SeaweedFS S3 API | published (`PORT_S3`) | browsers use presigned URLs against it |
| 9000 | Keycloak management | internal | `/health/ready` |
| 9333 | SeaweedFS master | internal | used by the bucket-creation job |
