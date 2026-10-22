# loamz-server

The open server side of Loamz: engines (`server/engine`), the data and agent servers (`server/server`), the document
reader (`server/reader`), the two web apps (`apps/`), OpenAPI specs (`specs/`) and deployment (`deploy/`).

- Python: one uv workspace at the root (`uv sync --all-packages --all-extras`, `uv run --package <name> ...`); `server/reader` is outside it (Python 3.13, own lock). Use `uv`, never `pip`. Ruff is the version pinned in `uv.lock`.
- The client libraries (`loamz-core`, `loamz-identity`, `loamz-data`) come from the loamz-python repo. This repo never edits them.
- JavaScript: one pnpm workspace at the root.
- Import contracts are in `.importlinter` (`make imports`): the engines import no server; the data server never imports the agent runtime; layers point downward.
- Never commit secrets: `.env*`, `*.pem`, `.workspace/` are ignored. Pushes and releases are done by the owner.
- No backward compatibility is needed (nothing is deployed): delete old code instead of shimming; APIs are versioned (`/api/v1`) and errors are RFC 9457.
