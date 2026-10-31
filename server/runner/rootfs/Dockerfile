# The sandbox's root filesystem: Python and DuckDB, nothing else that the code does not need. The sandbox mounts it read-only.
FROM python:3.12-slim
ARG DUCKDB_VERSION=2.0.0
# The Iceberg and S3 extensions are installed now, signed, into a fixed folder: a running sandbox has no network and cannot install any.
ENV DUCKDB_EXTENSIONS=/opt/duckdb-extensions
RUN pip install --no-cache-dir "duckdb==${DUCKDB_VERSION}" && rm -rf /root/.cache \
 && python -c "import duckdb,os; c=duckdb.connect(config={'extension_directory': os.environ['DUCKDB_EXTENSIONS']}); [c.execute(f'INSTALL {e}') for e in ('httpfs','avro','iceberg')]"
