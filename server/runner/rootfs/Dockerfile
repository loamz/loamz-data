# The sandbox's root filesystem: Python and DuckDB, nothing else that the code does not need. The sandbox mounts it read-only.
FROM python:3.12-slim
ARG DUCKDB_VERSION=2.0.0
RUN pip install --no-cache-dir "duckdb==${DUCKDB_VERSION}" && rm -rf /root/.cache
