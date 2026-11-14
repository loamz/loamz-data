from __future__ import annotations

import argparse
import json
import os
import sys
from typing import Any

from loamz.client import Client
from loamz.errors import LoamzError


def _format_table(headers: list[str], rows: list[list[Any]]) -> str:
    if not headers and not rows:
        return "(empty)"

    str_rows = [[str(val if val is not None else "") for val in row] for row in rows]
    widths = [len(h) for h in headers]
    for row in str_rows:
        for i, val in enumerate(row):
            if i < len(widths):
                widths[i] = max(widths[i], len(val))
            else:
                widths.append(len(val))

    header_line = "  ".join(h.ljust(widths[i]) for i, h in enumerate(headers))
    sep_line = "  ".join("-" * widths[i] for i in range(len(headers)))
    data_lines = [
        "  ".join(val.ljust(widths[i]) for i, val in enumerate(row)) for row in str_rows
    ]
    return "\n".join([header_line, sep_line, *data_lines])


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="loamz",
        description="Loamz Data CLI: inspect catalogs, run queries, and check identity.",
    )
    parser.add_argument(
        "--url",
        default=os.environ.get("LOAMZ_URL", "http://127.0.0.1:8020"),
        help="Base URL of Loamz Data API (default: LOAMZ_URL or http://127.0.0.1:8020)",
    )
    parser.add_argument(
        "--token",
        default=None,
        help="Bearer token (defaults to LOAMZ_TOKEN env var)",
    )
    parser.add_argument(
        "--workspace",
        "-w",
        default=None,
        help="Workspace name or ID (defaults to the first accessible workspace)",
    )

    common_parser = argparse.ArgumentParser(add_help=False)
    common_parser.add_argument(
        "--workspace",
        "-w",
        default=None,
        help="Workspace name or ID",
    )

    subparsers = parser.add_subparsers(dest="command", required=True)

    # whoami
    sub_whoami = subparsers.add_parser(
        "whoami", parents=[common_parser], help="Show current user identity and workspaces"
    )
    sub_whoami.add_argument("--json", action="store_true", help="Output as JSON")

    # ls [catalog[.schema]]
    sub_ls = subparsers.add_parser(
        "ls", parents=[common_parser], help="List catalogs, schemas, or tables"
    )
    sub_ls.add_argument(
        "target",
        nargs="?",
        default=None,
        help="Optional catalog or catalog.schema to inspect",
    )
    sub_ls.add_argument("--json", action="store_true", help="Output as JSON")

    # sql "<statement>"
    sub_sql = subparsers.add_parser(
        "sql", parents=[common_parser], help="Run a SQL statement against a workspace"
    )
    sub_sql.add_argument("statement", help="SQL statement to execute")
    sub_sql.add_argument("--limit", type=int, default=1000, help="Row limit (default: 1000)")
    sub_sql.add_argument("--json", action="store_true", help="Output as JSON")

    # import <file> <catalog.schema.table>
    sub_import = subparsers.add_parser(
        "import", parents=[common_parser], help="Bring a CSV or Parquet file in as a table"
    )
    sub_import.add_argument("file", help="A .csv or .parquet file on this machine")
    sub_import.add_argument("table", help="Where it goes, as catalog.schema.table")
    sub_import.add_argument("--append", action="store_true", help="Add the rows to a table that exists")

    # import-volume <volume> <path> <table>
    sub_vol = subparsers.add_parser(
        "import-volume",
        parents=[common_parser],
        help="Bring a file that is already in a volume in as a table (run on the workspace's compute)",
    )
    sub_vol.add_argument("volume", help="The volume, as catalog.schema.volume (or its id)")
    sub_vol.add_argument("path", help="The file's path inside the volume")
    sub_vol.add_argument("table", help="Where it goes, as catalog.schema.table")
    sub_vol.add_argument("--append", action="store_true", help="Add the rows to a table that exists")

    args = parser.parse_args(argv)

    try:
        client = Client(base_url=args.url, token=args.token)

        if args.command == "whoami":
            me = client.me()
            if args.json:
                data = {
                    "subject": me.subject,
                    "email": me.email,
                    "name": me.name,
                    "kind": me.kind,
                    "workspaces": [
                        {
                            "id": w.id,
                            "name": w.name,
                            "org_alias": w.org_alias,
                            "org_id": w.org_id,
                            "admin": w.admin,
                        }
                        for w in me.workspaces
                    ],
                }
                print(json.dumps(data, indent=2))
            else:
                print(f"Logged in as: {me.name} <{me.email}> ({me.subject})")
                if me.workspaces:
                    print("\nWorkspaces:")
                    headers = ["ID", "Name", "Organization", "Admin"]
                    rows = [[w.id, w.name, w.org_alias or w.org_id, str(w.admin)] for w in me.workspaces]
                    print(_format_table(headers, rows))
            return 0

        # Resolve workspace
        workspaces = client.workspaces()
        if not workspaces:
            print("Error: No workspaces available for this user.", file=sys.stderr)
            return 1

        if args.workspace:
            ws = client.workspace(args.workspace)
        else:
            # Default to first non-personal workspace, or first workspace
            non_personal = next((w for w in workspaces if w.org_alias != "Personal"), None)
            ws_item = non_personal or workspaces[0]
            ws = client.workspace(ws_item.id)

        if args.command == "ls":
            target = args.target
            if not target:
                catalogs = ws.catalogs()
                if args.json:
                    print(json.dumps([{"id": c.id, "name": c.name, "kind": c.kind} for c in catalogs], indent=2))
                else:
                    headers = ["Catalog", "ID", "Owner"]
                    rows = [[c.name, c.id, c.owner] for c in catalogs]
                    print(_format_table(headers, rows))
                return 0

            parts = target.split(".")
            if len(parts) == 1:
                # Target is catalog -> list schemas
                cat_name = parts[0]
                cats = ws.catalogs()
                cat = next((c for c in cats if c.name == cat_name or c.id == cat_name), None)
                if not cat:
                    print(f"Error: Catalog '{cat_name}' not found.", file=sys.stderr)
                    return 1
                data = client._send("GET", f"/api/v1/workspaces/{ws.id}/securables?parent_id={cat.id}")
                schemas = data.get("items", []) if isinstance(data, dict) else []
                if args.json:
                    print(json.dumps([{"id": s["id"], "name": s["name"], "kind": s["kind"]} for s in schemas], indent=2))
                else:
                    headers = ["Schema", "ID", "Owner"]
                    rows = [[s["name"], s["id"], s.get("owner", "")] for s in schemas]
                    print(_format_table(headers, rows))
                return 0

            if len(parts) >= 2:
                cat_name, schema_name = parts[0], parts[1]
                tables = ws.tables(cat_name, schema_name)
                if args.json:
                    print(json.dumps([{"id": t.id, "name": t.name, "kind": t.kind} for t in tables], indent=2))
                else:
                    headers = ["Table", "ID", "Owner"]
                    rows = [[t.name, t.id, t.owner] for t in tables]
                    print(_format_table(headers, rows))
                return 0

        if args.command == "import-volume":
            run = ws.import_from_volume(args.table, args.volume, args.path, mode="append" if args.append else "create")
            print((run.get("stdout") or "").strip() or f"imported into {args.table}")
            return 0

        if args.command == "import":
            if len(args.table.split(".")) != 3:
                print("Error: name the table as catalog.schema.table.", file=sys.stderr)
                return 1
            rows = ws.table(args.table).import_file(args.file, append=args.append)
            print(f"{rows} rows from {args.file} are in {args.table}")
            return 0

        if args.command == "sql":
            result = ws.sql(args.statement, limit=args.limit)
            if args.json:
                dict_rows = [dict(zip(result.columns, row)) for row in result.rows]
                print(json.dumps(dict_rows, indent=2))
            else:
                print(_format_table(result.columns, result.rows))
                print(f"\n({len(result.rows)} rows)")
            return 0

        return 0

    except LoamzError as e:
        print(f"Error ({e.status}): {e.detail or e.title}", file=sys.stderr)
        return 1
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
