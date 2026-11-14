import importlib.util
import pathlib
import unittest


SOURCE = pathlib.Path(__file__).parents[1] / "server/server/src/compute/maintenance_job.py"
SPEC = importlib.util.spec_from_file_location("maintenance_job", SOURCE)
maintenance = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(maintenance)


class Cursor:
    def __init__(self, rows, columns=()):
        self.rows = list(rows)
        self.description = [(column,) for column in columns]

    def fetchmany(self, size):
        rows, self.rows = self.rows[:size], self.rows[size:]
        return rows

    def fetchone(self):
        return self.rows[0] if self.rows else None

    def fetchall(self):
        return self.rows


class ChangedTable:
    def __init__(self, rewritten_rows=None, rewritten_count=2, added_count=1):
        self.statements = []
        self.rewritten_rows = rewritten_rows or [(1, "changed"), (2, "same")]
        self.rewritten_count = rewritten_count
        self.added_count = added_count
        self.file_list_calls = 0

    def execute(self, query, params=None):
        self.statements.append(query)
        if query.startswith("SELECT * FROM lake.raw.events"):
            return Cursor([(1, "before"), (2, "same")])
        if query.startswith("SELECT to_json(metadata)"):
            return Cursor([('{"location":"s3://bucket/table"}',)])
        if query.startswith("SELECT file FROM glob"):
            self.file_list_calls += 1
            files = [("old-1.parquet",), ("old-2.parquet",)]
            if self.file_list_calls > 1 and self.added_count:
                files.append(("new.parquet",))
            return Cursor(files)
        if query.startswith("CALL iceberg_rewrite_data_files"):
            return Cursor(
                [(self.rewritten_count, self.added_count, 100)],
                (
                    "rewritten_data_files",
                    "added_data_files",
                    "rewritten_bytes",
                ),
            )
        if query.startswith("SELECT * FROM read_parquet"):
            return Cursor(self.rewritten_rows)
        return Cursor([])


class SnapshotFilesConnection:
    def execute(self, query):
        if query.startswith("SELECT file_path FROM iceberg_files"):
            return Cursor([("s3://bucket/org/ws/table/data/live.parquet",)])
        if query.startswith("SELECT * FROM iceberg_manifests"):
            return Cursor([], ("content", "path"))
        if query.startswith("SELECT path FROM iceberg_manifests"):
            return Cursor([("s3://bucket/org/ws/table/metadata/manifest.avro",)])
        raise AssertionError(query)


class MaintenanceTests(unittest.TestCase):
    def test_checksum_mismatch_rolls_back_without_committing(self):
        con = ChangedTable()
        with self.assertRaisesRegex(RuntimeError, "compaction changed the table"):
            maintenance.compact(con, "lake.raw.events")
        self.assertIn("ROLLBACK", con.statements)
        self.assertNotIn("COMMIT", con.statements)

    def test_matching_rewritten_rows_commit(self):
        con = ChangedTable(rewritten_rows=[(1, "before"), (2, "same")])
        maintenance.compact(con, "lake.raw.events")
        self.assertIn("COMMIT", con.statements)
        self.assertNotIn("ROLLBACK", con.statements)

    def test_no_rewrite_rolls_back_without_scanning_new_files(self):
        con = ChangedTable(rewritten_count=0, added_count=0)
        maintenance.compact(con, "lake.raw.events")
        self.assertIn("ROLLBACK", con.statements)
        self.assertNotIn("COMMIT", con.statements)

    def test_expiry_keeps_files_referenced_by_a_live_snapshot(self):
        self.assertEqual(
            maintenance.paths_to_delete(
                {"old.parquet", "shared.parquet", "old.avro"},
                {"shared.parquet", "live.parquet"},
            ),
            ["old.avro", "old.parquet"],
        )

    def test_expiry_collects_live_data_and_manifest_paths(self):
        found = maintenance.snapshot_files(
            SnapshotFilesConnection(),
            'lake."raw"."events"',
            {
                "snapshot-id": 7,
                "manifest-list": "s3://bucket/org/ws/table/metadata/list.avro",
            },
            "bucket",
            "org/ws/table",
        )
        self.assertEqual(
            found,
            {
                "org/ws/table/data/live.parquet",
                "org/ws/table/metadata/manifest.avro",
                "org/ws/table/metadata/list.avro",
            },
        )

    def test_object_paths_cannot_escape_the_table_folder(self):
        with self.assertRaisesRegex(RuntimeError, "outside the table's folder"):
            maintenance.object_key("../other/data.parquet", "bucket", "workspace/table")


if __name__ == "__main__":
    unittest.main()
