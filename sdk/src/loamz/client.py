from __future__ import annotations

import os
import time
from pathlib import Path
from typing import Any

import httpx

from loamz.duck import TableHandle
from loamz.errors import LoamzError
from loamz.models import Me, Securable, Workspace
from loamz.sql import Rows


class Client:
    """Client for the Loamz Data API."""

    def __init__(
        self,
        base_url: str = "http://127.0.0.1:8020",
        *,
        token: str | None = None,
        token_file: str | Path | None = None,
    ) -> None:
        self.base_url = base_url.rstrip("/")

        # Token resolution: parameter -> LOAMZ_TOKEN -> token_file -> LOAMZ_TOKEN_FILE
        resolved_token = token or os.environ.get("LOAMZ_TOKEN")
        if not resolved_token:
            file_path = token_file or os.environ.get("LOAMZ_TOKEN_FILE")
            if file_path:
                p = Path(file_path).expanduser()
                if p.is_file():
                    resolved_token = p.read_text(encoding="utf-8").strip()

        self.token = resolved_token or ""
        headers = {
            "Accept": "application/json",
            "User-Agent": "loamz-python/0.1.0",
        }
        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"

        self._http = httpx.Client(
            base_url=self.base_url,
            headers=headers,
            timeout=120.0,
        )

    def close(self) -> None:
        self._http.close()

    def __enter__(self) -> Client:
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        self.close()

    def _send(self, method: str, path: str, *, json: Any = None) -> Any:
        response = self._http.request(method, path, json=json)
        if response.status_code >= 400:
            raise LoamzError.from_response(response)
        if response.status_code == 204 or not response.content:
            return None
        return response.json()

    def me(self) -> Me:
        """Fetch current user identity and accessible workspaces."""
        data = self._send("GET", "/api/v1/me")
        return Me.from_dict(data)

    def workspaces(self) -> list[Workspace]:
        """List all workspaces accessible to the user."""
        return self.me().workspaces

    def workspace(self, name_or_id: str) -> WorkspaceHandle:
        """Get a handle to a workspace by its id or name."""
        all_ws = self.workspaces()
        # First match exact ID
        for w in all_ws:
            if w.id == name_or_id:
                return WorkspaceHandle(w.id, client=self, name=w.name)
        # Next match exact name
        for w in all_ws:
            if w.name == name_or_id:
                return WorkspaceHandle(w.id, client=self, name=w.name)

        raise LoamzError(
            status=404,
            title="Not Found",
            detail=f"Workspace '{name_or_id}' was not found in accessible workspaces",
        )


class WorkspaceHandle:
    """Scoped workspace operations: catalogs, tables, and SQL execution."""

    def __init__(self, workspace_id: str, *, client: Client, name: str = "") -> None:
        self.id = workspace_id
        self.client = client
        self.name = name

    def __repr__(self) -> str:
        return f"WorkspaceHandle(id={self.id!r}, name={self.name!r})"

    def catalogs(self) -> list[Securable]:
        """List top-level catalogs in this workspace."""
        data = self.client._send("GET", f"/api/v1/workspaces/{self.id}/securables")
        items = data.get("items", []) if isinstance(data, dict) else []
        return [Securable.from_dict(item) for item in items]

    def tables(self, catalog: str, schema: str) -> list[Securable]:
        """List tables within a specific catalog and schema."""
        # 1. Resolve catalog ID
        catalogs = self.catalogs()
        cat = next((c for c in catalogs if c.id == catalog or c.name == catalog), None)
        if not cat:
            raise LoamzError(
                status=404,
                title="Not Found",
                detail=f"Catalog '{catalog}' not found in workspace '{self.id}'",
            )

        # 2. Resolve schema ID under catalog
        data = self.client._send(
            "GET", f"/api/v1/workspaces/{self.id}/securables?parent_id={cat.id}"
        )
        schemas = data.get("items", []) if isinstance(data, dict) else []
        sch = next((s for s in schemas if s["id"] == schema or s["name"] == schema), None)
        if not sch:
            raise LoamzError(
                status=404,
                title="Not Found",
                detail=f"Schema '{schema}' not found in catalog '{catalog}'",
            )

        # 3. List tables under schema
        data = self.client._send(
            "GET", f"/api/v1/workspaces/{self.id}/securables?parent_id={sch['id']}"
        )
        tables = data.get("items", []) if isinstance(data, dict) else []
        return [Securable.from_dict(t) for t in tables if t.get("kind") == "table"]

    def table(self, full_name: str) -> TableHandle:
        """Create a handle to an Iceberg table (e.g. 'catalog.schema.table')."""
        return TableHandle(
            full_name,
            workspace_id=self.id,
            base_url=self.client.base_url,
            token=self.client.token,
        )

    def volume_id(self, name_or_id: str) -> str:
        """The id of a volume, from its id or its full name `catalog.schema.volume`."""
        parts = name_or_id.split(".")
        if len(parts) != 3:
            return name_or_id
        parent = ""
        found: dict | None = None
        for name in parts:
            items = self.client._send("GET", f"/api/v1/workspaces/{self.id}/securables?parent_id={parent}").get("items", [])
            found = next((i for i in items if i["name"] == name), None)
            if found is None:
                raise LoamzError(status=404, title="Not Found", detail=f"{name_or_id} was not found")
            parent = found["id"]
        return parent

    def import_from_volume(
        self,
        target: str,
        volume: str,
        path: str,
        *,
        mode: str = "create",
        format: str | None = None,
        wait: bool = True,
    ) -> dict:
        """Bring a CSV or Parquet file that is already in a volume in as a table, run on this workspace's compute.

        ``target`` is ``catalog.schema.table``; ``mode`` is ``create`` (a new table) or ``append``. The server runs a program it
        wrote in a sandbox with no network, which reads the file and writes the table through the catalog. Returns the run (with
        ``rows`` in its output once it succeeded); with ``wait=False`` it returns at once with the run's id.
        """
        body = {"target": target, "volume": self.volume_id(volume), "path": path, "mode": mode}
        if format:
            body["format"] = format
        run = self.client._send("POST", f"/api/v1/workspaces/{self.id}/imports", json=body)
        if not wait:
            return run
        start = time.time()
        while run.get("state") in ("queued", "running"):
            if time.time() - start > 900:
                raise LoamzError(status=504, title="Gateway Timeout", detail="the import did not finish in 15 minutes")
            time.sleep(0.5)
            run = self.client._send("GET", f"/api/v1/workspaces/{self.id}/runs/{run['id']}")
        if run.get("state") != "succeeded":
            raise LoamzError(status=400, title="Import Failed", detail=run.get("error") or run.get("stderr") or "the import failed")
        return run

    def sql(
        self,
        statement: str,
        *,
        limit: int = 1000,
        wait_s: int = 30,
        target: str | None = None,
    ) -> Rows:
        """Run a SQL statement and wait for results."""
        body: dict[str, Any] = {
            "sql": statement,
            "limit": limit,
            "wait_s": min(wait_s, 60),
        }
        if target:
            body["target"] = target

        path = f"/api/v1/workspaces/{self.id}/sql/statements"
        response = self.client._http.post(path, json=body)
        if response.status_code >= 400:
            raise LoamzError.from_response(response)

        data = response.json() if response.content else {}
        run_id = data.get("id")

        state = data.get("state", "running")
        # If still running (status 202 or non-terminal state), poll /runs/{id}
        terminal_states = {"succeeded", "failed", "cancelled"}
        if response.status_code == 202 or state not in terminal_states:
            if not run_id:
                raise LoamzError(
                    status=500,
                    title="Internal Error",
                    detail="Server returned 202 Accepted without a run ID",
                )
            start_time = time.time()
            max_wait_seconds = 600.0  # Cap at 10 minutes
            while state not in terminal_states:
                if time.time() - start_time > max_wait_seconds:
                    raise LoamzError(
                        status=504,
                        title="Gateway Timeout",
                        detail=f"Query exceeded maximum wait time of {int(max_wait_seconds)}s",
                    )
                time.sleep(0.5)
                data = self.client._send("GET", f"/api/v1/workspaces/{self.id}/runs/{run_id}")
                state = data.get("state", "running")

        if state != "succeeded":
            err_msg = data.get("error") or f"Run ended in state '{state}'"
            raise LoamzError(status=400, title="Query Failed", detail=err_msg)

        result = data.get("result") or {}
        cols_raw = result.get("columns", [])
        columns = [c["name"] if isinstance(c, dict) else str(c) for c in cols_raw]
        rows = result.get("rows", [])
        return Rows(columns=columns, rows=rows)
