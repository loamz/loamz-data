# Runs as a job with one declared table write. Catalog commits and object deletes go through the two write-lease doors.
import hashlib
import json
import os
import sys
import time
import urllib.parse
import urllib.request

def row_checksum(con, source, params=None):
    digest = hashlib.sha256()
    query = f"SELECT * FROM {source} ORDER BY ALL"
    cursor = con.execute(query) if params is None else con.execute(query, params)
    count = 0
    while rows := cursor.fetchmany(1024):
        for row in rows:
            data = json.dumps(row, sort_keys=True, separators=(",", ":"), default=str).encode()
            digest.update(len(data).to_bytes(8, "big"))
            digest.update(data)
            count += 1
    return count, digest.hexdigest()


def data_files(con, location):
    return {
        row[0]
        for row in con.execute(
            "SELECT file FROM glob(?)", [location.rstrip("/") + "/data/**/*.parquet"]
        ).fetchall()
    }


def compact(con, table):
    con.execute("BEGIN TRANSACTION")
    try:
        before = row_checksum(con, table)
        metadata = json.loads(
            con.execute(
                f"SELECT to_json(metadata) FROM iceberg_load_table_response({table})"
            ).fetchone()[0]
        )
        before_files = data_files(con, metadata["location"])
        rewrite = con.execute(
            f"CALL iceberg_rewrite_data_files('{table}', min_input_files => 2)"
        )
        result = rewrite.fetchone()
        columns = [column[0] for column in rewrite.description]
        expected = {"rewritten_data_files", "added_data_files", "rewritten_bytes"}
        if result is None or not expected.issubset(columns):
            raise RuntimeError(f"compaction returned unexpected results: {columns}, {result}")
        stats = dict(zip(columns, result))
        if stats["rewritten_data_files"] == 0:
            con.execute("ROLLBACK")
            return before, stats
        after_files = data_files(con, metadata["location"])
        rewritten_files = sorted(after_files - before_files)
        if len(rewritten_files) != stats["added_data_files"]:
            raise RuntimeError(
                "compaction created an unexpected number of data files: "
                f"expected {stats['added_data_files']}, found {len(rewritten_files)}"
            )
        after = row_checksum(con, "read_parquet(?)", [rewritten_files])
        if before != after:
            raise RuntimeError(
                f"compaction changed the table: count {before[0]} to {after[0]}, "
                f"checksum {before[1]} to {after[1]}"
            )
        con.execute("COMMIT")
        return before, stats
    except Exception as error:
        try:
            con.execute("ROLLBACK")
        except Exception as rollback_error:
            raise RuntimeError(f"{error}; transaction rollback also failed: {rollback_error}") from error
        raise


def paths_to_delete(expired_paths, live_paths):
    return sorted(set(expired_paths) - set(live_paths))


def object_key(path, bucket, prefix):
    if path.startswith("s3://"):
        parsed = urllib.parse.urlsplit(path)
        if parsed.netloc != bucket:
            raise RuntimeError("an Iceberg file is outside the table's bucket")
        key = urllib.parse.unquote(parsed.path).lstrip("/")
    else:
        relative = path.lstrip("/")
        key = relative if relative.startswith(prefix + "/") else prefix + "/" + relative
    if not key.startswith(prefix + "/") or any(part in (".", "..") for part in key.split("/")):
        raise RuntimeError("an Iceberg file is outside the table's folder")
    return key


def snapshot_files(con, table, snapshot, bucket, prefix):
    snapshot_id = int(snapshot["snapshot-id"])
    paths = set()
    manifest_list = snapshot.get("manifest-list")
    if manifest_list:
        paths.add(object_key(manifest_list, bucket, prefix))
    query = f"SELECT file_path FROM iceberg_files({table}, snapshot_from_id => {snapshot_id})"
    for row in con.execute(query).fetchall():
        paths.add(object_key(row[0], bucket, prefix))

    manifests = f"iceberg_manifests({table}, snapshot_from_id => {snapshot_id})"
    columns = [
        column[0]
        for column in con.execute(f"SELECT * FROM {manifests} LIMIT 0").description
    ]
    if "path" not in columns:
        raise RuntimeError(f"iceberg_manifests returned no path column: {columns}")
    for row in con.execute(f"SELECT path FROM {manifests}").fetchall():
        paths.add(object_key(row[0], bucket, prefix))
    return paths


def send_expiry(catalog_endpoint, catalog, schema, table, metadata, snapshot_ids):
    import loamz_doors

    path = (
        f"{catalog_endpoint}/v1/{urllib.parse.quote(catalog)}/namespaces/"
        f"{urllib.parse.quote(schema)}/tables/{urllib.parse.quote(table)}"
    )
    requirements = [
        {"type": "assert-table-uuid", "uuid": metadata["table-uuid"]},
        {
            "type": "assert-ref-snapshot-id",
            "ref": "main",
            "snapshot-id": metadata.get("refs", {}).get("main", {}).get("snapshot-id"),
        },
    ]
    body = json.dumps(
        {
            "requirements": requirements,
            "updates": [{"action": "remove-snapshots", "snapshot-ids": snapshot_ids}],
        }
    ).encode()
    request = urllib.request.Request(
        path, data=body, headers={"Content-Type": "application/json"}, method="POST"
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        response.read()


def expire(con, table, catalog, schema, name, workspace, catalog_endpoint, storage, older_than):
    _, raw = con.execute(
        f"SELECT metadata_location, to_json(metadata) "
        f"FROM iceberg_load_table_response({table})"
    ).fetchone()
    metadata = json.loads(raw)
    snapshots = metadata.get("snapshots", [])
    current = metadata.get("current-snapshot-id")
    refs = {
        int(ref["snapshot-id"])
        for ref in metadata.get("refs", {}).values()
        if ref.get("snapshot-id") is not None
    }
    cutoff = time.time_ns() // 1_000_000 - older_than * 1000
    keep = []
    remove = []
    for snapshot in snapshots:
        snapshot_id = int(snapshot["snapshot-id"])
        at = snapshot.get("timestamp-ms")
        if (
            snapshot_id == current
            or snapshot_id in refs
            or at is None
            or int(at) >= cutoff
        ):
            keep.append(snapshot)
        else:
            remove.append(snapshot)
    if not remove:
        return 0

    location = urllib.parse.urlsplit(metadata["location"])
    if location.scheme != "s3" or not location.netloc or not location.path:
        raise RuntimeError("the table is not stored in the configured object store")
    bucket = location.netloc
    prefix = urllib.parse.unquote(location.path).strip("/")
    expired_paths = set()
    for snapshot in remove:
        expired_paths.update(snapshot_files(con, table, snapshot, bucket, prefix))
    live_paths = set()
    for snapshot in keep:
        live_paths.update(snapshot_files(con, table, snapshot, bucket, prefix))

    send_expiry(
        catalog_endpoint,
        catalog,
        schema,
        name,
        metadata,
        [int(snapshot["snapshot-id"]) for snapshot in remove],
    )

    for key in paths_to_delete(expired_paths, live_paths):
        url = f"http://{storage}/{urllib.parse.quote(bucket, safe='')}/{urllib.parse.quote(key, safe='/')}"
        request = urllib.request.Request(url, method="DELETE")
        with urllib.request.urlopen(request, timeout=30) as response:
            response.read()
    return len(remove)


def main():
    spec = json.loads(bytes.fromhex("__SPEC__").decode())
    catalog, schema, name = spec["target"].split(".")
    table = f'lake."{schema}"."{name}"'
    sys.path.insert(0, "/inputs/job")
    import duckdb
    import loamz_doors

    catalog_endpoint, storage_endpoint = loamz_doors.start(spec["workspace"])
    con = duckdb.connect(
        ":memory:",
        config={
            "extension_directory": os.environ.get("DUCKDB_EXTENSIONS", "/opt/duckdb-extensions"),
            "autoinstall_known_extensions": "false",
            "autoload_known_extensions": "false",
        },
    )
    con.execute("LOAD httpfs")
    con.execute("LOAD iceberg")
    con.execute(
        f"CREATE SECRET door (TYPE s3, KEY_ID 'door', SECRET 'door', ENDPOINT '{storage_endpoint}', URL_STYLE 'path', USE_SSL false, REGION 'auto')"
    )
    con.execute(
        f"ATTACH '{catalog}' AS lake (TYPE iceberg, ENDPOINT '{catalog_endpoint}', TOKEN 'door', ACCESS_DELEGATION_MODE 'none')"
    )

    if spec["action"] == "compact":
        (rows, checksum), stats = compact(con, table)
        print(
            json.dumps(
                {
                    "action": "compact",
                    "rows": rows,
                    "checksum": checksum,
                    "files_rewritten": stats["rewritten_data_files"],
                    "files_added": stats["added_data_files"],
                    "bytes_rewritten": stats["rewritten_bytes"],
                }
            )
        )
    else:
        removed = expire(
            con,
            table,
            catalog,
            schema,
            name,
            spec["workspace"],
            catalog_endpoint,
            storage_endpoint,
            spec["older_than"],
        )
        print(json.dumps({"action": "expire", "snapshots_removed": removed}))


if __name__ == "__main__":
    main()
