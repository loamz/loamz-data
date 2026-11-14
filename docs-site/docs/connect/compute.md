# Connecting Compute Machines & Runners

Loamz Data sells governance only: compute statements and transformations run on your own infrastructure—a dedicated VM or VPS, a developer laptop, or your own serverless cloud account.

Compute nodes establish outbound HTTPS connections only to your Loamz Data workspace. They require no inbound ports and hold no long-term storage credentials.

---

## Architecture Overview

Loamz Data supports three operational compute modes:

| Mode | Environment | Isolation & Governance | Ideal For |
|---|---|---|---|
| **Dedicated VM / VPS** | Ubuntu 22.04+ / Debian 12+ (Root) | Full kernel sandboxing via gVisor `runsc`. Memory, CPU, network, file, and timeout limits strictly enforced. | Production workloads, scheduled ETL, multi-user teams. |
| **Local Laptop** | Linux workstation or WSL2 (Rootless) | Filesystem, network, and timeout sandboxing. Process and memory limits are **not** enforced. | Local query development and fast prototyping. |
| **Serverless Cloud** | Modal account | Ephemeral containers deployed into your cloud account. Auto-scales from zero, scales to zero when idle. | Burst queries, cost-optimized analytics, ad-hoc BI. |

---

## Mode 1: Dedicated VM or VPS (Root Execution)

In this mode, `loamz-runner` runs under systemd with root privileges, delegating query execution to `loamz-worker` running as `nobody` inside a locked-down gVisor sandbox. Memory and CPU quotas are strictly enforced by the host cgroup hierarchy.

### 1. Generate an Agent Secret

In the Loamz Data Console:
1. Navigate to **Compute & Runners** in your workspace.
2. Click **Add Compute Target** and choose **Agent Machine**.
3. Copy the enrollment secret (`agt_...`) shown once in the confirmation dialog.

### 2. Run the One-Command Installer

On a clean Ubuntu 24.04 or Debian 12 machine:

```bash
# Option A: Run installer with secret argument
sudo ./install.sh --url https://data.loamz.com --secret agt_0123456789abcdef...

# Option B: Run installer and provide secret via hidden prompt (avoids shell history)
sudo ./install.sh --url https://data.loamz.com
```

The installer performs the following idempotent actions:
- Validates the host operating system (Ubuntu / Debian).
- Creates a dedicated system user `loamz`.
- Unpacks binaries into `/opt/loamz-agent/bin` (`loamz-runner`, `loamz-worker`, `libduckdb.so`, `runsc`).
- Extracts the verified sandbox container rootfs to `/opt/loamz-agent/rootfs`.
- Writes `/etc/loamz-agent/env` and writes `/etc/loamz-agent/secret` with restricted mode `0600`.
- Configures and starts the systemd service `loamz-agent.service` (`Restart=on-failure`, `NoNewPrivileges=false`).
- Verifies runner startup and prints a health line:
  ```text
  loamz-agent: healthy and listening for work from https://data.loamz.com (platform: systrap)
  ```

### 3. Uninstalling

To cleanly uninstall the agent, remove all binaries, services, and credentials:

```bash
sudo /opt/loamz-agent/install.sh --uninstall
```

---

## Mode 2: Laptop Without Root (Rootless Development)

If you are developing locally on a laptop or workstation where you do not have root or `sudo` access, you can run the agent directly in unprivileged rootless mode.

### Setup

1. Unpack the agent bundle into your home directory:
   ```bash
   mkdir -p ~/loamz-agent
   tar -xzf loamz-agent-x86_64.tar.gz -C ~/loamz-agent
   tar --zstd -xf ~/loamz-agent/rootfs.tar.zst -C ~/loamz-agent/rootfs
   ```

2. Store your agent enrollment secret in a secure file:
   ```bash
   mkdir -p ~/.config/loamz
   echo "agt_..." > ~/.config/loamz/agent.secret
   chmod 600 ~/.config/loamz/agent.secret
   ```

3. Start the agent:
   ```bash
   export LOAMZ_DATA_URL="https://data.loamz.com"
   export LOAMZ_AGENT_SECRET_FILE="$HOME/.config/loamz/agent.secret"
   export LOAMZ_RUNSC="$HOME/loamz-agent/runsc"
   export LOAMZ_ROOTFS="$HOME/loamz-agent/rootfs"
   export LOAMZ_WORK="$HOME/.cache/loamz/work"
   export LOAMZ_PLATFORM="ptrace"

   ~/loamz-agent/loamz-runner agent
   ```

### Honest Security Caveats for Rootless Mode

When running without root access, the runner prints this warning upon startup:

```text
WARN loamz_runner: not running as root: gVisor cannot enforce the memory and process limits here (the network, file and time limits still hold)
```

**What this means:**
- **What is still enforced:** The sandbox cannot access your host filesystem, cannot open arbitrary network sockets, cannot read ungranted tables, and strictly respects statement wall-clock deadlines.
- **What is NOT enforced:** Because unprivileged users cannot write to root cgroups on the host, a runaway query in rootless mode can consume all available RAM on your laptop or spawn excessive internal threads until killed by the OS OOM killer or query deadline.

---

## Mode 3: Serverless Compute via Modal

For elastic compute that scales to zero when no queries are executing, you can deploy a serverless compute endpoint into your own [Modal](https://modal.com) account.

### 1. Set Up the Modal Secret

Create the `loamz-sql` secret in your Modal account containing the shared authentication bearer token:

```bash
modal secret create loamz-sql LOAMZ_TOKEN="your-secure-token-here"
```

### 2. Deploy the Modal SQL Endpoint

Using the template in `server/serverless/modal_app.py`:

```bash
# Choose the Modal region closest to your object storage bucket
LOAMZ_SQL_REGION=us-east modal deploy server/serverless/modal_app.py
```

Modal outputs your deployed function endpoint URL:
```text
✓ Created web endpoint: https://<organization>--loamz-sql-sql.modal.run
```

### 3. Attach the Serverless Target to Loamz Data

Attach the endpoint via the Loamz Data API or console:

```bash
curl -X POST "https://data.loamz.com/api/v1/workspaces/<workspace-id>/compute/targets" \
  -H "Authorization: Bearer <user-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "modal-serverless-east",
    "kind": "serverless",
    "endpoint": "https://<organization>--loamz-sql-sql.modal.run",
    "max_memory_mb": 2048,
    "max_cpu_millis": 1000
  }'
```

### How Serverless Execution Operates

- **Zero-Storage Footprint:** The Modal container receives short-lived, read-only S3 storage credentials scoped exclusively to the tables queried in each specific statement.
- **Immediate Tear-Down:** After 60 seconds of inactivity, Modal automatically scales the container down to zero.
- **Hard Memory Caps:** DuckDB memory is strictly bounded by the `memory_mb` limit passed in each statement request.
