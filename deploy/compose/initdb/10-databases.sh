#!/bin/bash
# Runs once, on first start of the Postgres volume. POSTGRES_DB (loamz_data) already exists.
set -euo pipefail
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname postgres <<'SQL'
CREATE DATABASE keycloak;
CREATE DATABASE data_retrieval;
CREATE DATABASE loamz_compute;
SQL
for db in "$POSTGRES_DB" data_retrieval; do
  psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$db" <<'SQL'
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
SQL
done
