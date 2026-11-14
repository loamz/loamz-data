"""Governed data for code that runs in a Loamz sandbox.

The sandbox has no network and no credential. The volumes this run may read are reached through one local socket to the machine's
storage broker, which serves only those folders, only for reading, by byte range (so Polars, DuckDB and pyarrow fetch just the parts
of a file they use) and remembers query results it has already read.

    import loamz, polars as pl
    df = pl.scan_parquet(loamz.path("agent_files", "threads/t1/results/q.parquet")).filter(pl.col("x") > 3).collect()
    con = loamz.duckdb()                                  # DuckDB, already pointed at the volumes
    con.execute(f"SELECT count(*) FROM read_parquet('{loamz.path('agent_files', 'threads/t1/results/q.parquet')}')").fetchall()
    local = loamz.fetch("agent_files", "uploads/report.xlsx")   # one file copied to /tmp, for libraries that need a local path

What you write goes in /workspace; it is local and disappears with the sandbox.
"""
import json
import os
import pkgutil
import socket
import threading
import urllib.request

# `loamz.data` (the SDK) is another package under the same name: keep it importable.
__path__ = pkgutil.extend_path(__path__, __name__)

_CONFIG = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "loamz.json")
_BROKER = "/inputs/broker/socket"
_state = {"port": None}


def _pump(source, target):
    try:
        while True:
            data = source.recv(65536)
            if not data:
                break
            target.sendall(data)
    except OSError:
        pass
    finally:
        try:
            target.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def _bridge():
    """A port on 127.0.0.1 in front of the broker's socket: what the readers' HTTP clients can connect to."""
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("127.0.0.1", 0))
    server.listen(64)

    def serve():
        while True:
            client, _ = server.accept()
            door = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            try:
                door.connect(_BROKER)
            except OSError:
                client.close()
                continue
            threading.Thread(target=_pump, args=(client, door), daemon=True).start()
            threading.Thread(target=_pump, args=(door, client), daemon=True).start()

    threading.Thread(target=serve, daemon=True).start()
    return server.getsockname()[1]


def _config():
    with open(_CONFIG) as f:
        return json.load(f)


def endpoint():
    """`127.0.0.1:<port>`: the address readers use as their S3 endpoint."""
    if _state["port"] is None:
        _state["port"] = _bridge()
        # Polars (and anything built on the same object-store library) finds its settings in the environment.
        os.environ.update(
            AWS_ENDPOINT_URL=f"http://127.0.0.1:{_state['port']}",
            AWS_ACCESS_KEY_ID="placeholder",
            AWS_SECRET_ACCESS_KEY="placeholder",
            AWS_REGION=_config().get("region", "auto"),
            AWS_DEFAULT_REGION=_config().get("region", "auto"),
            AWS_ALLOW_HTTP="true",
            AWS_VIRTUAL_HOSTED_STYLE_REQUEST="false",
        )
    return f"127.0.0.1:{_state['port']}"


def volumes():
    """The names of the volumes this run may read."""
    return sorted(_config()["volumes"])


def path(volume, relative=""):
    """The `s3://` address of a file (or folder) in a volume this run may read."""
    config = _config()
    if volume not in config["volumes"]:
        raise KeyError(f"this run may not read {volume!r}; it may read {sorted(config['volumes'])}")
    parts = [p for p in str(relative).split("/") if p]
    if ".." in parts:
        raise ValueError("a path inside a volume has no '..'")
    endpoint()
    return "s3://" + "/".join([config["bucket"], config["volumes"][volume].strip("/"), *parts])


def duckdb():
    """A DuckDB connection that reads the volumes through the broker."""
    import duckdb as _duckdb

    con = _duckdb.connect(
        ":memory:",
        config={
            "extension_directory": os.environ.get("DUCKDB_EXTENSIONS", "/opt/duckdb-extensions"),
            "autoinstall_known_extensions": "false",
            "autoload_known_extensions": "false",
        },
    )
    con.execute("LOAD httpfs")
    con.execute(
        f"CREATE SECRET (TYPE s3, KEY_ID 'placeholder', SECRET 'placeholder', ENDPOINT '{endpoint()}', URL_STYLE 'path', USE_SSL false, REGION 'auto')"
    )
    return con


def fs():
    """A pyarrow filesystem over the volumes: `pyarrow.parquet.read_table(loamz.path(...)[5:], filesystem=loamz.fs())`."""
    import pyarrow.fs as pafs

    return pafs.S3FileSystem(
        endpoint_override=endpoint(), scheme="http", access_key="placeholder", secret_key="placeholder", region="auto"
    )


def fetch(volume, relative):
    """Copies one file of a volume to /tmp and returns its local path: for libraries that cannot read an `s3://` address. Only that
    file is read, not the volume."""
    address = path(volume, relative)[len("s3://"):]
    target = os.path.join("/tmp", "loamz-fetch", volume, *[p for p in str(relative).split("/") if p])
    os.makedirs(os.path.dirname(target), exist_ok=True)
    with urllib.request.urlopen(f"http://{endpoint()}/{address}", timeout=600) as response, open(target, "wb") as out:
        while chunk := response.read(1 << 20):
            out.write(chunk)
    return target
