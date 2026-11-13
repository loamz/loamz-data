# Compute Console

The Compute console manages query engines, machine runners, and sandbox execution targets in your workspace.

---

## Compute Models

Loamz Data supports three execution modes:

1. **Direct Engine (Client-Side)**:
   - External tools (DuckDB, Apache Spark, Trino, Python SDK) connect directly to the Iceberg REST catalog.
   - Zero compute costs on the Loamz server: engines read data files straight from object storage with vended credentials.
2. **Serverless Warehouses**:
   - On-demand query execution for web console queries, scheduled dashboards, and API callers.
   - Automatically pauses when idle to eliminate waste.
3. **Dedicated Machine Runners**:
   - Customer-hosted compute agents (`loamz-runner agent`) running on your own servers or cloud instances.
   - Queries execute inside hardened gVisor (`runsc`) sandboxes with strict CPU, memory, and network boundaries.

---

## Attaching a Machine Runner

To attach a private compute machine to your workspace:
1. Navigate to **Compute** $\rightarrow$ **Attach Machine**.
2. Name the target (e.g. `prod-runner-01`) and define resource caps (CPU millicores and memory MB).
3. The console displays the machine's enrollment secret (`agt_...`). Save this secret to a secure file.
4. Start the runner on your host:
   ```bash
   export LOAMZ_DATA_URL="https://data.example.com"
   export LOAMZ_AGENT_SECRET_FILE="/etc/loamz/agent.secret"
   loamz-runner agent
   ```
5. The machine status switches to **Online** and begins picking up assigned query jobs.
