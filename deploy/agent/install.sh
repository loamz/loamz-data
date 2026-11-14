#!/usr/bin/env bash
# Loamz Compute Agent Installer
# Attaches a compute machine to Loamz Data under systemd.
#
# Usage:
#   sudo ./install.sh --url http://127.0.0.1:8020
#   sudo ./install.sh --url https://data.example.com --secret agt_...
#   sudo ./install.sh --uninstall
set -euo pipefail

UNINSTALL=0
DATA_URL="${LOAMZ_DATA_URL:-}"
SECRET="${LOAMZ_AGENT_SECRET:-}"
SECRET_FILE=""
BUNDLE_PATH=""
PLATFORM="${LOAMZ_PLATFORM:-}"

while [ $# -gt 0 ]; do
  case "$1" in
    --uninstall)
      UNINSTALL=1
      shift
      ;;
    --url)
      DATA_URL="$2"
      shift 2
      ;;
    --secret)
      SECRET="$2"
      shift 2
      ;;
    --secret-file)
      SECRET_FILE="$2"
      shift 2
      ;;
    --bundle)
      BUNDLE_PATH="$2"
      shift 2
      ;;
    --platform)
      PLATFORM="$2"
      shift 2
      ;;
    -h|--help)
      cat <<EOF
Usage: install.sh [OPTIONS]

Options:
  --url URL           Loamz Data server URL (e.g. https://data.example.com or http://127.0.0.1:8020)
  --secret SECRET     Agent secret token (agt_...). If omitted, you will be prompted securely.
  --secret-file PATH  Read agent secret from file
  --platform NAME     gVisor platform: systrap or ptrace (auto-detected by default)
  --bundle PATH       Path or URL to loamz-agent-<arch>.tar.gz bundle
  --uninstall         Remove loamz-agent, its service, binaries, and configurations
EOF
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

# Check OS: Ubuntu or Debian
if [ -f /etc/os-release ]; then
  # shellcheck source=/dev/null
  . /etc/os-release
  case "${ID:-}" in
    ubuntu|debian) ;;
    *)
      if [[ "${ID_LIKE:-}" != *"debian"* && "${ID_LIKE:-}" != *"ubuntu"* ]]; then
        echo "Error: loamz-agent requires Ubuntu or Debian Linux (detected: ${ID:-unknown})." >&2
        exit 1
      fi
      ;;
  esac
else
  echo "Error: cannot verify OS from /etc/os-release." >&2
  exit 1
fi

# Ensure running as root or re-execute with sudo
if [ "$(id -u)" -ne 0 ]; then
  if command -v sudo >/dev/null 2>&1; then
    exec sudo -E "$0" "$@"
  else
    echo "Error: install.sh must run as root or with sudo." >&2
    exit 1
  fi
fi

# Handle uninstall
if [ "$UNINSTALL" -eq 1 ]; then
  echo "Uninstalling loamz-agent..."
  if command -v systemctl >/dev/null 2>&1; then
    systemctl stop loamz-agent.service 2>/dev/null || true
    systemctl disable loamz-agent.service 2>/dev/null || true
    rm -f /etc/systemd/system/loamz-agent.service
    systemctl daemon-reload 2>/dev/null || true
  fi
  pkill -f "loamz-runner agent" 2>/dev/null || true
  rm -rf /opt/loamz-agent /etc/loamz-agent /var/lib/loamz-agent
  userdel loamz 2>/dev/null || true
  echo "loamz-agent uninstalled successfully."
  exit 0
fi

# Ensure data URL is provided
if [ -z "$DATA_URL" ]; then
  if [ -t 0 ]; then
    read -r -p "Enter Loamz Data server URL (e.g. http://127.0.0.1:8020): " DATA_URL
  else
    echo "Error: --url <DATA_URL> is required." >&2
    exit 1
  fi
fi
DATA_URL="${DATA_URL%/}"

# Acquire secret securely if not already given
if [ -n "$SECRET_FILE" ] && [ -f "$SECRET_FILE" ]; then
  SECRET="$(cat "$SECRET_FILE")"
fi

if [ -z "$SECRET" ]; then
  if [ -t 0 ]; then
    read -r -s -p "Enter Loamz Agent Secret (agt_...): " SECRET
    echo ""
  else
    echo "Error: --secret <SECRET> or --secret-file <PATH> is required in non-interactive mode." >&2
    exit 1
  fi
fi
SECRET="$(printf "%s" "$SECRET" | tr -d ' \r\n')"

if [[ ! "$SECRET" =~ ^agt_ ]]; then
  echo "Error: the agent secret must begin with 'agt_'." >&2
  exit 1
fi

# Create loamz system user if missing
if ! id -u loamz >/dev/null 2>&1; then
  useradd --system --shell /usr/sbin/nologin --home-dir /opt/loamz-agent --no-create-home loamz
fi

# Find bundle source
HERE="$(cd "$(dirname "$0")" && pwd)"
TEMP_DIR=""
cleanup() {
  if [ -n "$TEMP_DIR" ] && [ -d "$TEMP_DIR" ]; then
    rm -rf "$TEMP_DIR"
  fi
}
trap cleanup EXIT

SOURCE_DIR=""
if [ -f "$HERE/loamz-runner" ] && [ -f "$HERE/loamz-worker" ] && [ -f "$HERE/runsc" ]; then
  SOURCE_DIR="$HERE"
elif [ -n "$BUNDLE_PATH" ] && [ -f "$BUNDLE_PATH" ]; then
  TEMP_DIR="$(mktemp -d)"
  tar -xzf "$BUNDLE_PATH" -C "$TEMP_DIR"
  SOURCE_DIR="$TEMP_DIR"
elif [ -n "$BUNDLE_PATH" ] && [[ "$BUNDLE_PATH" =~ ^https?:// ]]; then
  TEMP_DIR="$(mktemp -d)"
  curl -fsSL "$BUNDLE_PATH" | tar -xz -C "$TEMP_DIR"
  SOURCE_DIR="$TEMP_DIR"
else
  # Check if a bundle exists in current directory or target/agent
  ARCH="$(uname -m)"
  LOCAL_BUNDLE="loamz-agent-${ARCH}.tar.gz"
  if [ -f "$HERE/$LOCAL_BUNDLE" ]; then
    TEMP_DIR="$(mktemp -d)"
    tar -xzf "$HERE/$LOCAL_BUNDLE" -C "$TEMP_DIR"
    SOURCE_DIR="$TEMP_DIR"
  elif [ -f "$HERE/../target/agent/$LOCAL_BUNDLE" ]; then
    TEMP_DIR="$(mktemp -d)"
    tar -xzf "$HERE/../target/agent/$LOCAL_BUNDLE" -C "$TEMP_DIR"
    SOURCE_DIR="$TEMP_DIR"
  elif [ -n "$DATA_URL" ]; then
    # Attempt download from server URL
    TEMP_DIR="$(mktemp -d)"
    DOWNLOAD_URL="${DATA_URL}/downloads/agent/${LOCAL_BUNDLE}"
    echo "Downloading agent bundle from ${DOWNLOAD_URL}..."
    if curl -fsSL -o "$TEMP_DIR/$LOCAL_BUNDLE" "$DOWNLOAD_URL"; then
      tar -xzf "$TEMP_DIR/$LOCAL_BUNDLE" -C "$TEMP_DIR"
      SOURCE_DIR="$TEMP_DIR"
    else
      echo "Error: bundle files not found beside installer, and could not download from ${DOWNLOAD_URL}." >&2
      exit 1
    fi
  else
    echo "Error: cannot find bundle files or loamz-agent-${ARCH}.tar.gz." >&2
    exit 1
  fi
fi

# Prepare directories
mkdir -p /opt/loamz-agent/bin /opt/loamz-agent/rootfs /var/lib/loamz-agent/work /etc/loamz-agent

# Install binaries
cp -p "$SOURCE_DIR/loamz-runner" /opt/loamz-agent/bin/
cp -p "$SOURCE_DIR/loamz-worker" /opt/loamz-agent/bin/
cp -p "$SOURCE_DIR/libduckdb.so" /opt/loamz-agent/bin/
cp -p "$SOURCE_DIR/runsc" /opt/loamz-agent/bin/
if [ -f "$SOURCE_DIR/install.sh" ]; then
  cp -p "$SOURCE_DIR/install.sh" /opt/loamz-agent/
  cp -p "$SOURCE_DIR/install.sh" /opt/loamz-agent/bin/
fi
chmod 755 /opt/loamz-agent/bin/* /opt/loamz-agent/install.sh 2>/dev/null || chmod 755 /opt/loamz-agent/bin/*

# Unpack rootfs.tar.zst into /opt/loamz-agent/rootfs
if [ -f "$SOURCE_DIR/rootfs.tar.zst" ]; then
  if command -v apt-get >/dev/null 2>&1; then
    PKGS=""
    command -v zstd >/dev/null 2>&1 || PKGS="$PKGS zstd"
    if [ ! -d /etc/ssl/certs ] || [ -z "$(ls -A /etc/ssl/certs 2>/dev/null)" ]; then
      PKGS="$PKGS ca-certificates"
    fi
    if [ -n "$PKGS" ]; then
      apt-get update -qq && apt-get install -y -qq $PKGS
    fi
  fi
  # Unpack rootfs (remove old rootfs first if needed)
  if [ ! -x /opt/loamz-agent/rootfs/usr/local/bin/python3 ]; then
    mkdir -p /opt/loamz-agent/rootfs
    tar --zstd -xf "$SOURCE_DIR/rootfs.tar.zst" -C /opt/loamz-agent/rootfs
  fi
fi

if [ ! -x /opt/loamz-agent/rootfs/usr/local/bin/python3 ]; then
  echo "Error: sandbox rootfs missing /usr/local/bin/python3" >&2
  exit 1
fi

# Determine gVisor platform (systrap or ptrace)
if [ -z "$PLATFORM" ]; then
  if /opt/loamz-agent/bin/runsc --platform=systrap do true >/dev/null 2>&1; then
    PLATFORM="systrap"
  else
    PLATFORM="ptrace"
  fi
fi

# Write environment configuration
cat <<EOF > /etc/loamz-agent/env
LOAMZ_DATA_URL=${DATA_URL}
LOAMZ_RUNSC=/opt/loamz-agent/bin/runsc
LOAMZ_ROOTFS=/opt/loamz-agent/rootfs
LOAMZ_WORK=/var/lib/loamz-agent/work
LOAMZ_AGENT_SECRET_FILE=/etc/loamz-agent/secret
LOAMZ_PLATFORM=${PLATFORM}
EOF
chmod 644 /etc/loamz-agent/env

# Write agent secret (mode 0600)
(
  umask 077
  printf "%s\n" "$SECRET" > /etc/loamz-agent/secret
)
chown root:loamz /etc/loamz-agent/secret
chmod 600 /etc/loamz-agent/secret

# Permissions on working directory
chown -R root:loamz /opt/loamz-agent
chown -R loamz:loamz /var/lib/loamz-agent
chmod 775 /var/lib/loamz-agent/work

# Install and start systemd service
SERVICE_SRC="$SOURCE_DIR/loamz-agent.service"
if [ ! -f "$SERVICE_SRC" ] && [ -f "$HERE/loamz-agent.service" ]; then
  SERVICE_SRC="$HERE/loamz-agent.service"
fi

if [ -f "$SERVICE_SRC" ] && command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
  cp "$SERVICE_SRC" /etc/systemd/system/loamz-agent.service
  systemctl daemon-reload
  systemctl enable loamz-agent.service
  systemctl restart loamz-agent.service
  # Wait briefly for start
  sleep 1
fi

echo "loamz-agent: healthy and listening for work from ${DATA_URL} (platform: ${PLATFORM})"
