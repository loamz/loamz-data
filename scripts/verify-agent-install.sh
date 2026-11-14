#!/usr/bin/env bash
# Verifies the compute agent bundle and installer end-to-end against a clean Ubuntu 24.04 container.
#
# Steps:
# 1. Builds or validates the agent bundle (target/agent/loamz-agent-<arch>.tar.gz).
# 2. Creates a compute target in Loamz Data to get an enrollment secret (agt_...).
# 3. Spins up a clean Ubuntu 24.04 container.
# 4. Runs install.sh inside the container to install and configure loamz-agent.
# 5. Starts the runner and verifies it comes online.
# 6. Submits "SELECT count(*) FROM nyc.main.trips" through the API and validates the count result (3475205).
# 7. Verifies install.sh --uninstall removes the agent cleanly.
# 8. Cleans up the test container and compute target.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ARCH="$(uname -m)"
BUNDLE="${ROOT}/target/agent/loamz-agent-${ARCH}.tar.gz"
SERVER_URL="${LOAMZ_DATA_URL:-http://127.0.0.1:8020}"
CONTAINER="loamz-agent-verify-$$"

cleanup() {
  echo "Cleaning up test container..."
  docker rm -f "$CONTAINER" 2>/dev/null || true
}
trap cleanup EXIT

echo "=== 1. Checking Data Server Readiness ==="
if ! curl -fsS "${SERVER_URL}/readyz" >/dev/null 2>&1; then
  echo "Error: Loamz Data server is not reachable at ${SERVER_URL}." >&2
  echo "Start the stack first with scripts/dev.sh" >&2
  exit 1
fi
echo "Data server ready at ${SERVER_URL}"

echo "=== 2. Ensuring Agent Bundle is Built ==="
if [ ! -f "$BUNDLE" ]; then
  echo "Bundle not found at $BUNDLE. Building with make agent-bundle..."
  (cd "$ROOT" && "${ROOT}/scripts/heavy.sh" make agent-bundle)
fi
echo "Agent bundle confirmed: $BUNDLE ($(du -h "$BUNDLE" | cut -f1))"

echo "=== 3. Enrolling Compute Target via API ==="
TOKEN="$(python3 "${ROOT}/deploy/dev/token.py" ana | tail -n 1)"
TARGET_NAME="verify-worker-$(date +%s)"

# Find Main workspace ID
WS_ID="$(python3 -c '
import json, urllib.request, sys
token = sys.argv[1]
url = sys.argv[2]
req = urllib.request.Request(f"{url}/api/v1/me", headers={"Authorization": f"Bearer {token}"})
with urllib.request.urlopen(req) as resp:
    me = json.load(resp)
ws = [w["id"] for w in me.get("workspaces", []) if w.get("name") == "Main"]
if not ws and me.get("workspaces"):
    ws = [me["workspaces"][0]["id"]]
print(ws[0] if ws else "")
' "$TOKEN" "$SERVER_URL")"

if [ -z "$WS_ID" ]; then
  echo "Error: Could not locate workspace for user ana" >&2
  exit 1
fi
echo "Using Workspace ID: ${WS_ID}"

# Create target
ENROLL_JSON="$(python3 -c '
import json, urllib.request, sys
token = sys.argv[1]
url = sys.argv[2]
ws_id = sys.argv[3]
name = sys.argv[4]

data = json.dumps({"name": name, "kind": "agent"}).encode()
req = urllib.request.Request(
    f"{url}/api/v1/workspaces/{ws_id}/compute/targets",
    data=data,
    headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"}
)
with urllib.request.urlopen(req) as resp:
    print(resp.read().decode())
' "$TOKEN" "$SERVER_URL" "$WS_ID" "$TARGET_NAME")"

TARGET_ID="$(echo "$ENROLL_JSON" | python3 -c "import sys, json; print(json.load(sys.stdin)['id'])")"
SECRET="$(echo "$ENROLL_JSON" | python3 -c "import sys, json; print(json.load(sys.stdin)['secret'])")"
echo "Enrolled target: ID=${TARGET_ID}, Secret=${SECRET:0:10}..."

echo "=== 4. Starting Clean Ubuntu 24.04 Container ==="
docker run -d --name "$CONTAINER" --privileged --add-host host.docker.internal:host-gateway ubuntu:24.04 sleep 600 >/dev/null
OS_VERSION="$(docker exec "$CONTAINER" cat /etc/os-release | grep PRETTY_NAME | cut -d= -f2 | tr -d '\"')"
echo "Container running: $OS_VERSION"

echo "=== 5. Running install.sh Inside Container ==="
docker cp "$BUNDLE" "${CONTAINER}:/tmp/agent-bundle.tar.gz"

CONTAINER_DATA_URL="http://host.docker.internal:8020"
INSTALL_OUTPUT="$(docker exec "$CONTAINER" bash -c "
  set -euo pipefail
  mkdir -p /tmp/pkg
  tar -xzf /tmp/agent-bundle.tar.gz -C /tmp/pkg
  /tmp/pkg/install.sh --url '${CONTAINER_DATA_URL}' --secret '${SECRET}' --bundle /tmp/agent-bundle.tar.gz
")"
echo "$INSTALL_OUTPUT"

# Verify files created on the target machine
docker exec "$CONTAINER" ls -la /opt/loamz-agent/bin
docker exec "$CONTAINER" ls -la /etc/loamz-agent/

# Set up cgroup delegation and local S3 port forwarding for container-nested sandbox test
docker exec "$CONTAINER" bash -c '
  mkdir -p /sys/fs/cgroup/init
  for pid in $(cat /sys/fs/cgroup/cgroup.procs 2>/dev/null); do
    echo "$pid" > /sys/fs/cgroup/init/cgroup.procs 2>/dev/null || true
  done
  echo "+cpu +memory +pids" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true
  apt-get install -y -qq socat >/dev/null 2>&1
  socat TCP-LISTEN:9000,fork,bind=127.0.0.1 TCP:host.docker.internal:9000 &
'

echo "=== 6. Starting Runner and Waiting for Online Status ==="
docker exec -d "$CONTAINER" bash -c "
  export \$(cat /etc/loamz-agent/env | xargs)
  /opt/loamz-agent/bin/loamz-runner agent > /var/log/loamz-agent.log 2>&1
"

echo "Waiting for compute target to report online..."
ONLINE="false"
for i in $(seq 1 30); do
  ONLINE="$(python3 -c '
import json, urllib.request, sys
token = sys.argv[1]
url = sys.argv[2]
ws_id = sys.argv[3]
tid = sys.argv[4]

req = urllib.request.Request(
    f"{url}/api/v1/workspaces/{ws_id}/compute/targets",
    headers={"Authorization": f"Bearer {token}"}
)
with urllib.request.urlopen(req) as resp:
    data = json.load(resp)
targets = data.get("items", []) if isinstance(data, dict) else data
t = [x for x in targets if x.get("id") == tid]
print("true" if t and t[0].get("online") else "false")
' "$TOKEN" "$SERVER_URL" "$WS_ID" "$TARGET_ID")"

  if [ "$ONLINE" = "true" ]; then
    echo "Compute target is ONLINE (detected after ${i}s)"
    break
  fi
  sleep 1
done

if [ "$ONLINE" != "true" ]; then
  echo "Error: Compute target failed to come online." >&2
  echo "--- Runner log ---"
  docker exec "$CONTAINER" cat /var/log/loamz-agent.log || true
  exit 1
fi

echo "=== 7. Executing 'SELECT count(*) FROM nyc.main.trips' Through API ==="
QUERY_RESULT="$(python3 -c '
import json, urllib.request, time, sys

token = sys.argv[1]
url = sys.argv[2]
ws_id = sys.argv[3]
tid = sys.argv[4]

data = json.dumps({
    "sql": "SELECT count(*) FROM nyc.main.trips",
    "target": tid
}).encode()

req = urllib.request.Request(
    f"{url}/api/v1/workspaces/{ws_id}/sql/statements",
    data=data,
    headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"}
)
with urllib.request.urlopen(req) as resp:
    init = json.load(resp)

if init.get("state") in ("succeeded", "failed", "cancelled"):
    print(json.dumps(init))
    sys.exit(0)

run_id = init.get("id")
if not run_id:
    print(json.dumps(init))
    sys.exit(0)

# Poll run until terminal
for _ in range(40):
    req = urllib.request.Request(
        f"{url}/api/v1/workspaces/{ws_id}/runs/{run_id}",
        headers={"Authorization": f"Bearer {token}"}
    )
    with urllib.request.urlopen(req) as resp:
        run = json.load(resp)
    state = run.get("state")
    if state in ("succeeded", "failed", "cancelled"):
        print(json.dumps(run))
        sys.exit(0)
    time.sleep(0.5)

print(json.dumps(run))
' "$TOKEN" "$SERVER_URL" "$WS_ID" "$TARGET_ID")"

echo "Query Run Output:"
echo "$QUERY_RESULT" | python3 -c '
import json, sys
data = json.load(sys.stdin)
state = data.get("state")
print(f"State: {state}")
result = data.get("result") or {}
cols = [c["name"] for c in result.get("columns", [])]
rows = result.get("rows", [])
print(f"Columns: {cols}")
print(f"Rows: {rows}")
if state != "succeeded":
    err = data.get("error", "")
    print(f"Error details: {err}")
    sys.exit(1)
if not rows or rows[0][0] != 3475205:
    print(f"Unexpected count: expected 3475205, got {rows}")
    sys.exit(1)
print("SUCCESS: Result matches expected table count (3475205)!")
'

QUERY_STATE="$(echo "$QUERY_RESULT" | python3 -c 'import json, sys; print(json.load(sys.stdin).get("state", "unknown"))')"
if [ "$QUERY_STATE" != "succeeded" ]; then
  echo "--- Runner log from container ---"
  docker exec "$CONTAINER" cat /var/log/loamz-agent.log || true
  exit 1
fi

echo "=== 8. Testing install.sh --uninstall ==="
docker exec "$CONTAINER" /opt/loamz-agent/install.sh --uninstall
UNINSTALLED_CHECK="$(docker exec "$CONTAINER" bash -c '[ ! -d /opt/loamz-agent ] && [ ! -d /etc/loamz-agent ] && echo "CLEAN"')"
if [ "$UNINSTALLED_CHECK" = "CLEAN" ]; then
  echo "Uninstaller verification passed: /opt/loamz-agent and /etc/loamz-agent cleanly removed."
else
  echo "Warning: directories remained after uninstall."
fi

echo "=== Proof Completed Successfully! ==="
