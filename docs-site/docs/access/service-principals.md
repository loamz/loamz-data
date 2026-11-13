# Service Principals and Automation

Service principals are non-human accounts used by CI/CD pipelines, automated ETL jobs, BI dashboards, and AI agents to authenticate with Loamz Data.

---

## Creating a Service Principal

1. In your workspace, navigate to **Access** $\rightarrow$ **Service Principals**.
2. Click **+ Create Service Principal**.
3. Provide a name (e.g. `dbt-runner`, `airflow-loader`, `bi-tableau`).
4. Generate an API token. Copy the secret key immediately; it is shown only once and cannot be retrieved later.

---

## Authenticating with a Service Principal

Pass the service principal token as a standard HTTP Bearer token:

```bash
curl -H "Authorization: Bearer <service-principal-token>" \
     https://data.example.com/api/v1/me
```

In the Python SDK:

```python
from loamz import Client

client = Client("https://data.example.com", token="<service-principal-token>")
ws = client.workspace("Main")
rows = ws.sql("SELECT count(*) FROM analytics.production.daily_metrics")
```

---

## Delegated Agent Access (RFC 8693)

AI agents such as Loamz Agents connect to Loamz Data using OAuth 2.0 Token Exchange (RFC 8693). The agent exchanges its primary identity token for a scoped Loamz Data token, acting on behalf of the user while preserving the human caller's exact identity and permission boundaries in the audit log.
