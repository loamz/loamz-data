# Managing People and Team Access

Loamz Data integrates with your organization's identity provider (OIDC / Keycloak) to manage team members and collaborative access.

---

## Inviting Members

Organization administrators can add people to the organization and assign them to specific workspaces:
1. Open the Organization settings or Workspace **Access** tab.
2. Enter the user's email address and select their initial workspace role:
   - **Workspace Admin**: Full control over catalogs, compute targets, team permissions, and workspace settings.
   - **Workspace Member**: Can query catalogs, create tables in assigned schemas, and execute SQL statements.

---

## User Groups

Groups simplify permission assignment across teams (e.g. `data-engineers`, `analytics-team`):
1. In the Organization directory, create a group and add user members.
2. In your workspace, assign table or schema permissions to the group rather than individual accounts.
3. When team members join or leave the group, their data access updates immediately.
