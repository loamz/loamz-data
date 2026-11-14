#!/usr/bin/env bash
# Packages the Loamz Compute Agent bundle: loamz-agent-<arch>.tar.gz
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ARCH="$(uname -m)"
AGENT_DIR="${ROOT}/target/agent"
STAGING_DIR="${AGENT_DIR}/staging"
BUNDLE_ARCHIVE="${AGENT_DIR}/loamz-agent-${ARCH}.tar.gz"

echo "Building Loamz Agent bundle for ${ARCH}..."
mkdir -p "${AGENT_DIR}"
rm -rf "${STAGING_DIR}"
mkdir -p "${STAGING_DIR}"

# 1. Ensure loamz-runner is built
if [ ! -x "${ROOT}/target/release/loamz-runner" ]; then
  echo "Building loamz-runner..."
  cargo build --release --locked -p loamz-runner
fi
cp -p "${ROOT}/target/release/loamz-runner" "${STAGING_DIR}/loamz-runner"

# 2. Ensure loamz-worker and libduckdb.so exist
if [ ! -x "${ROOT}/target/release/loamz-worker" ] || [ ! -f "${ROOT}/target/release/libduckdb.so" ]; then
  echo "Building SQL worker and fetching libduckdb.so..."
  make worker
fi
cp -p "${ROOT}/target/release/loamz-worker" "${STAGING_DIR}/loamz-worker"
cp -p "${ROOT}/target/release/libduckdb.so" "${STAGING_DIR}/libduckdb.so"

# 3. Ensure pinned runsc exists (with checksum validation)
RUNSC_PATH="${HOME}/.cache/loamz/runsc"
if [ ! -x "${RUNSC_PATH}" ]; then
  echo "Fetching pinned runsc..."
  "${ROOT}/scripts/fetch-runsc.sh" "${RUNSC_PATH}"
fi
cp -p "${RUNSC_PATH}" "${STAGING_DIR}/runsc"

# 4. Ensure sandbox rootfs exists as rootfs.tar.zst
ROOTFS_DIR="${HOME}/.cache/loamz/rootfs"
CACHED_ROOTFS_TAR="${HOME}/.cache/loamz/rootfs.tar.zst"

if [ ! -d "${ROOTFS_DIR}" ] || [ ! -x "${ROOTFS_DIR}/usr/local/bin/python3" ]; then
  echo "Building sandbox rootfs..."
  "${ROOT}/server/runner/rootfs/build.sh" "${ROOTFS_DIR}"
fi

if [ ! -f "${CACHED_ROOTFS_TAR}" ] || [ "${ROOTFS_DIR}" -nt "${CACHED_ROOTFS_TAR}" ]; then
  echo "Compressing sandbox rootfs to rootfs.tar.zst..."
  tar -cf - -C "${ROOTFS_DIR}" . | zstd -3 -T0 -q -o "${CACHED_ROOTFS_TAR}.part"
  mv "${CACHED_ROOTFS_TAR}.part" "${CACHED_ROOTFS_TAR}"
fi
cp -p "${CACHED_ROOTFS_TAR}" "${STAGING_DIR}/rootfs.tar.zst"

# 5. Installer and systemd unit
cp -p "${ROOT}/deploy/agent/install.sh" "${STAGING_DIR}/install.sh"
chmod +x "${STAGING_DIR}/install.sh"
cp -p "${ROOT}/deploy/agent/loamz-agent.service" "${STAGING_DIR}/loamz-agent.service"

# 6. Create bundle archive
echo "Creating archive ${BUNDLE_ARCHIVE}..."
tar -czf "${BUNDLE_ARCHIVE}" -C "${STAGING_DIR}" .
rm -rf "${STAGING_DIR}"

SIZE="$(du -h "${BUNDLE_ARCHIVE}" | cut -f1)"
echo "Agent bundle ready: ${BUNDLE_ARCHIVE} (${SIZE})"
