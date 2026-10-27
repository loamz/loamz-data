# Loamz Data

The data platform: catalog, SQL, files, documents, memory and databases. `server/engine` is the embeddable storage engine
(`loamz-engine`, package `loamz.engine`), `server/server` the platform (`loamz-data-server`, `loamz.server.data`),
`server/reader` the document reader, `apps/data-workspace` the web workspace and `specs/data` the API contract.

Loamz Data knows nothing of Loamz Agents. Token verification comes from `loamz-identity[server]` in
[loamz-python](https://github.com/loamz/loamz-python). Split out of `loamz-server` with its history kept; see `SPLIT.md`.

    uv sync --all-packages --all-groups
    make test
    uv run lint-imports --config .importlinter
