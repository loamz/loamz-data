"""Restore drill: prove a backup of the catalog database restores into a working, intact catalog, and time it.

    uv run --package loamz-server python deploy/scripts/restore_drill.py \\
        --source postgresql://loamz:…@db:5432/loamz_data --admin postgresql://loamz:…@db:5432/postgres

It dumps the source database (`pg_dump -Fc`), creates a scratch database next to it, restores into that (`pg_restore`),
and then checks the restored copy with the application's own code: every governed table has the same row count as the
source, and the audit log's hash chain verifies (so nothing was lost or altered in the backup path). The scratch database
is dropped afterwards. It prints the elapsed time of each step (your measured recovery time for this database) and exits
non-zero on any mismatch.

`--seed` first fills an empty source with a small catalog and audit trail, for scheduled runs that have no real data.
Needs `pg_dump`, `pg_restore` and `psql` of the server's major version on the PATH.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from urllib.parse import urlsplit, urlunsplit

from loamz.server.data.kernel.metadata import CatalogStore
from loamz.server.data.kernel.principal import Principal

COUNTED = (
    "catalogs",
    "schemas",
    "tables",
    "grants",
    "audit_events",
    "legal_holds",
    "access_tokens",
)


def with_database(url: str, name: str) -> str:
    parts = urlsplit(url)
    return urlunsplit(parts._replace(path="/" + name))


def run(*command: str) -> None:
    done = subprocess.run(command, capture_output=True, text=True)
    if done.returncode:
        sys.exit(f"FAIL {' '.join(command[:2])}: {done.stderr.strip()[:500]}")


def counts(url: str) -> dict[str, int]:
    store = CatalogStore(url=url, pool_size=2)
    try:
        with store.connection() as db:
            return {
                t: db.execute(f"SELECT count(*) AS n FROM {t}").fetchone()["n"] for t in COUNTED
            }
    finally:
        store.close()


def seed(url: str) -> None:
    store = CatalogStore(url=url, pool_size=2)
    try:
        store.ensure_default("data-admins")
        admin = Principal(subject="drill", name="drill", kind="user", is_admin=True)
        catalog = store.create_catalog("drill", admin.id, "restore drill")
        for i in range(5):
            store.create_schema(catalog, f"s{i}", admin.id, None)
        for i in range(200):
            store.audit(admin, "drill.event", f"securable-{i}", "success", f"req-{i}", n=i)
    finally:
        store.close()


def main() -> None:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--source", required=True, help="URL of the catalog database to back up")
    ap.add_argument(
        "--admin",
        required=True,
        help="URL of a maintenance database on the same server (to create the scratch one)",
    )
    ap.add_argument("--seed", action="store_true", help="fill the source with sample data first")
    ap.add_argument("--keep", action="store_true", help="do not drop the scratch database")
    ap.add_argument(
        "--prove-detection",
        action="store_true",
        help="also alter one audit event in the restored copy and require the chain check to catch it",
    )
    args = ap.parse_args()

    scratch = "drill_" + re.sub(r"\W", "", str(int(time.time())))
    target = with_database(args.admin, scratch)
    timings: dict[str, float] = {}

    if args.seed:
        seed(args.source)
    before = counts(args.source)

    with tempfile.TemporaryDirectory() as folder:
        dump = Path(folder) / "catalog.dump"
        started = time.perf_counter()
        run("pg_dump", "--format=custom", "--no-owner", "--file", str(dump), args.source)
        timings["backup"] = time.perf_counter() - started

        run("psql", args.admin, "-v", "ON_ERROR_STOP=1", "-c", f'CREATE DATABASE "{scratch}"')
        try:
            started = time.perf_counter()
            run("pg_restore", "--no-owner", "--exit-on-error", "--dbname", target, str(dump))
            timings["restore"] = time.perf_counter() - started

            started = time.perf_counter()
            after = counts(target)
            store = CatalogStore(url=target, pool_size=2)
            try:
                chain = store.verify_audit()
            finally:
                store.close()
            timings["verify"] = time.perf_counter() - started
            if args.prove_detection:
                tampered = CatalogStore(url=target, pool_size=2)
                try:
                    with tampered.connection() as db:
                        db.execute(
                            "UPDATE audit_events SET action='tampered' WHERE id=(SELECT min(id) FROM audit_events)"
                        )
                    caught = not tampered.verify_audit()["ok"]
                finally:
                    tampered.close()
                print(f"tamper   {'detected' if caught else 'NOT DETECTED'}")
                if not caught:
                    sys.exit("FAIL the audit chain did not notice an altered event")
        finally:
            if not args.keep:
                run("psql", args.admin, "-c", f'DROP DATABASE IF EXISTS "{scratch}" WITH (FORCE)')

    bad = {t: (before[t], after[t]) for t in COUNTED if before[t] != after[t]}
    for step, seconds in timings.items():
        print(f"{step:8} {seconds:7.2f}s")
    print(f"rows     {sum(after.values())} across {len(COUNTED)} governed tables")
    print(
        f"audit    {chain['events_checked']} events checked, chain {'intact' if chain['ok'] else 'BROKEN'}"
    )
    if bad:
        sys.exit(f"FAIL row counts differ after restore (source, restored): {bad}")
    if not chain["ok"]:
        sys.exit(f"FAIL audit chain broken at event {chain['first_bad_id']}")
    print(f"PASS restored in {timings['restore'] + timings['verify']:.1f}s")


if __name__ == "__main__":
    main()
