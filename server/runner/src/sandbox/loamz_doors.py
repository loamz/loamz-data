"""The two doors of a write lease, as local addresses a program inside the sandbox can use.

The sandbox has no network. A run that writes tables is given two Unix sockets in /inputs/doors instead: one that reaches the table
catalog, one that reaches storage. DuckDB speaks TCP, so each socket is put behind a port on 127.0.0.1 here. Nothing else leaves
the sandbox, and the doors themselves decide what is allowed (see server/runner/src/door.rs).
"""
import socket
import threading

DOORS = "/inputs/doors"


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


def _bridge(path):
    """Listens on a free local port and passes each connection to the Unix socket at `path`. Returns the port."""
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("127.0.0.1", 0))
    server.listen(64)

    def serve():
        while True:
            client, _ = server.accept()
            door = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            try:
                door.connect(path)
            except OSError:
                client.close()
                continue
            threading.Thread(target=_pump, args=(client, door), daemon=True).start()
            threading.Thread(target=_pump, args=(door, client), daemon=True).start()

    threading.Thread(target=serve, daemon=True).start()
    return server.getsockname()[1]


def start(workspace):
    """Returns (catalog endpoint, storage endpoint): the ENDPOINT of an Iceberg ATTACH, and the ENDPOINT of an S3 secret (host:port)."""
    catalog = _bridge(f"{DOORS}/catalog.sock")
    storage = _bridge(f"{DOORS}/storage.sock")
    return f"http://127.0.0.1:{catalog}/api/v1/workspaces/{workspace}/iceberg", f"127.0.0.1:{storage}"
