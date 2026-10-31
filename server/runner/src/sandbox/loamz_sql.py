"""What answering a SQL statement means, the same wherever it runs (the sandbox on a machine, or a serverless endpoint): how a value
becomes JSON, and how rows are read and capped. Written by Loamz; nothing a person supplies is run here except the statement itself,
which DuckDB runs after the caller has locked it down."""

import datetime
import decimal
import json

MAX_BYTES = 3_500_000
SAFE_INT = 2**53


def q(name):
    """A name as a quoted SQL identifier."""
    return '"' + name.replace('"', '""') + '"'


def lit(text):
    """Text as a SQL string literal."""
    return "'" + text.replace("'", "''") + "'"


def cell(v):
    """One value as JSON: integers beyond what a browser holds exactly, and anything unusual, become text."""
    if v is None or isinstance(v, (bool, str)):
        return v
    if isinstance(v, int):
        return v if abs(v) < SAFE_INT else str(v)
    if isinstance(v, float):
        return v if v == v and v not in (float("inf"), float("-inf")) else str(v)
    if isinstance(v, (bytes, bytearray)):
        return v.hex()
    if isinstance(v, (datetime.datetime, datetime.date, datetime.time)):
        return v.isoformat()
    if isinstance(v, (list, tuple)):
        return [cell(x) for x in v]
    if isinstance(v, dict):
        return {str(k): cell(x) for k, x in v.items()}
    if isinstance(v, decimal.Decimal):
        return str(v)
    return str(v)


def answer(con, sql, limit):
    """Runs the statement and returns {"columns", "rows", "truncated"}, or {"error"}: a mistake in the SQL is an answer."""
    try:
        relation = con.sql(sql)
        columns = [{"name": n, "type": str(t)} for n, t in zip(relation.columns, relation.types)]
        rows = relation.limit(limit + 1).fetchall()
    except Exception as e:  # noqa: BLE001
        return {"error": str(e)}
    truncated = len(rows) > limit
    out, size = [], 0
    for r in rows[:limit]:
        row = [cell(v) for v in r]
        size += len(json.dumps(row))
        if size > MAX_BYTES:
            truncated = True
            break
        out.append(row)
    return {"columns": columns, "rows": out, "truncated": truncated}
