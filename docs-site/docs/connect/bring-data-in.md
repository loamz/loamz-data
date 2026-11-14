# Bring Data In

Loamz Data supports importing CSV and Parquet files into managed Apache Iceberg tables: through the interactive web console, directly from your workstation with `loamz import`, or as a sandboxed compute job from a volume or a web address with `loamz import-volume` or the import API.

---

## 1. Web Console Import

You can import files directly in the browser through the Catalog console.

### Import Sources

The console import dialog offers two source options:
- **From a volume**: Choose an existing CSV or Parquet file from any accessible volume, or upload a new file on the fly.
- **From a web address**: Download and import data from a public `https://` address directly to compute.

### Import Modes

When importing into a table, you can select one of three modes:
- **New table (`create`)**: Creates a new Iceberg table matching the schema inferred from the input file. The table must not already exist.
- **Add rows (`append`)**: Appends the rows from the file to an existing table.
- **Replace rows (`replace`)**: Atomically replaces all existing rows in the table with the imported rows within an Iceberg transaction. The existing table schema and column definitions are preserved. The table must already exist.

> [!WARNING]
> Selecting **Replace rows** removes all current rows from the target table before inserting the new data.

### Importing from a Web Address

When providing a web address:
1. Enter the full `https://` URL (e.g. `https://data.example.com/monthly_report.parquet` or an S3 presigned URL).
2. If the URL does not end with `.csv` or `.parquet` (common with signed URLs containing query signatures), explicitly choose the **File format** (CSV or Parquet).
3. The URL is transmitted securely to the compute worker. For privacy and security, signed query strings and URLs are **never echoed back** in the UI or stored in run metadata logs.

#### Address Restrictions & Security Policies

To prevent server-side request forgery (SSRF) and protect internal networks, compute workers enforce strict network validation before and during file downloads:
- **HTTPS Only**: Only `https://` schemes are permitted. Plain `http://` is rejected.
- **Port 443 Only**: Requests to non-standard ports (such as `https://example.com:8443/...`) are refused.
- **No Credentials**: Userinfo in the URL (e.g. `https://user:pass@example.com/...`) is prohibited.
- **Public IPs Only**: The target hostname is resolved via DNS and its IP address is validated:
  - **Loopback addresses** (`127.0.0.0/8`, `::1`) are refused.
  - **Private networks** (RFC 1918: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`) are refused.
  - **Link-local addresses** (`169.254.0.0/16`, `fe80::/10`) and cloud metadata endpoints are refused.
  - **Carrier-grade NAT** (`100.64.0.0/10`), broadcast, documentation, and non-global IPv6 ranges are refused.
- **Redirects**: At most 3 redirects are followed, each strictly validated against these same IP and port constraints.
- **Resource Limits**: Download size is capped at 2 GiB and download time at 10 minutes.

### Step-by-Step: Import from a Volume File

1. In the **Catalog**, navigate to a volume (e.g. `nyc.main.uploads`).
2. On any CSV or Parquet file row, click **Import as table**.
3. In the import dialog:
   - **Target Schema**: Select the catalog and schema (e.g. `nyc.main`).
   - **Table Name**: Enter a name (lower-case letters, digits, and underscores, starting with a letter).
   - **Mode**: Choose **New table**, **Add rows**, or **Replace rows**.
4. Click **Start import**. The server runs an import job on your workspace compute. Once finished, the dialog displays the imported row count and provides a link to view the table.

### Step-by-Step: Import from a Table Page

1. Open any table in the **Catalog**.
2. Click **Import from a file** in the top action bar.
3. Select the source: **From a volume** (with a volume file or file upload) or **From a web address**.
4. Mode defaults to **Add rows**, or select **Replace rows** if you wish to overwrite existing table data.
5. Click **Start import** to trigger the compute run and monitor its progress in real time.

---

## 2. Direct Storage Import: `loamz import`

When working on your local machine with the Loamz CLI or Python SDK, `loamz import` reads local files and writes them directly into the table's object store using short-lived vended credentials from the catalog:

```sh
# Create a new Iceberg table from a local Parquet or CSV file
loamz import trips.parquet nyc.main.trips

# Append rows to an existing table
loamz import more.csv nyc.main.trips --append

# Replace all rows in an existing table
loamz import fresh.csv nyc.main.trips --replace
```

Using the Python SDK:

```python
from loamz import Client

client = Client()
ws = client.workspace("Main")

# Import directly to storage (mode="create", "append", or "replace")
rows = ws.table("nyc.main.trips").import_file("trips.parquet", mode="replace")
print(f"Replaced table data with {rows} rows")
```

The file data is streamed directly to cloud storage without traversing the control plane. You need `CREATE TABLE` on the schema or `MODIFY` on the table.

---

## 3. Volume Import on Compute: `loamz import-volume`

Files already located in a volume (uploaded via console, SDK, or generated by an upstream job) can be imported by compute runners inside your private network without requiring local DuckDB or external S3 access:

```sh
# Import from a volume file to create a new table
loamz import-volume uploads.main.incoming trips.csv nyc.main.trips

# Append from a volume file
loamz import-volume uploads.main.incoming more.csv nyc.main.trips --append

# Replace rows from a volume file
loamz import-volume uploads.main.incoming fresh.csv nyc.main.trips --replace
```

Using the Python SDK:

```python
from loamz import Client

client = Client()
ws = client.workspace("Main")

# Launch asynchronous import job on workspace compute
run = ws.import_from_volume("nyc.main.trips", "uploads.main.incoming", "trips.csv", mode="replace")
run.wait()
print(f"Run {run.id} finished with status {run.state}")
```

The job runs in a sandboxed compute container reaching catalog and storage exclusively through governed loopback doors.

---

## Permissions Required

| Action | Required Privileges |
|---|---|
| Create new table from volume file | `READ` on volume, `CREATE TABLE` on target schema |
| Append rows to table from volume file | `READ` on volume, `MODIFY` on target table |
| Replace rows in table from volume file | `READ` on volume, `MODIFY` on target table |
| Import from web address | `CREATE TABLE` on target schema or `MODIFY` on target table |
| Direct import (`loamz import`) | `CREATE TABLE` on target schema (or `MODIFY` on table) |
