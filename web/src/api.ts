// The Data API, one function per call. Errors are RFC 9457 problem documents; they become an `ApiError` carrying the
// server's own words, which are written to be shown to a person.

import { signOut, token } from "./auth";

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

export type Kind = "catalog" | "schema" | "table" | "volume" | "view";
export type Role = "reader" | "contributor" | "owner";

export interface OrgInfo {
  id: string;
  alias: string;
  kind: "org" | "personal";
  has_account: boolean;
  account_admin: boolean;
  can_create_account: boolean;
}
export interface WorkspaceInfo {
  id: string;
  name: string;
  org_id: string;
  org_alias: string;
  admin: boolean;
}
export interface Me {
  subject: string;
  name: string;
  email: string;
  kind: "person" | "service";
  groups: string[];
  agent: string | null;
  orgs: OrgInfo[];
  workspaces: WorkspaceInfo[];
}
export interface Workspace {
  id: string;
  org_id: string;
  name: string;
  created_ms: number;
  admin: boolean;
  account_admin: boolean;
}
export interface Member {
  principal: string;
  role: "admin" | "user";
}
export interface Securable {
  id: string;
  kind: Kind;
  parent_id: string;
  name: string;
  owner: string;
  classification: string;
  placement: string;
  consistency: string;
  created_ms: number;
}
export interface Held {
  principal: string;
  role: Role | null;
  privileges: string[];
}
export interface FileEntry {
  path: string;
  size: number;
}
export interface AuditEntry {
  seq: number;
  at_ms: number;
  actor: string;
  on_behalf_of: string;
  action: string;
  target: string;
  outcome: string;
}
export interface Task {
  id: string;
  kind: string;
  state: "pending" | "done" | "failed";
  attempts: number;
  last_error: string;
}
export interface Verdict {
  intact: boolean;
  entries: number;
  first_broken: number | null;
}

/** Reads an error answer into words. Anything that is not a problem document gets a plain sentence for its status. */
export async function readError(res: Response): Promise<ApiError> {
  try {
    const p = (await res.json()) as { detail?: unknown };
    if (typeof p.detail === "string" && p.detail) return new ApiError(res.status, p.detail);
  } catch {
    /* not JSON */
  }
  return new ApiError(res.status, res.status >= 500 ? "Something went wrong on our side." : `The request failed (${res.status}).`);
}

async function send(method: string, path: string, body?: BodyInit | object, headers: Record<string, string> = {}): Promise<Response> {
  const t = await token();
  if (!t) {
    await signOut();
    throw new ApiError(401, "Sign in to continue.");
  }
  const isJson = body !== undefined && !(body instanceof Blob) && typeof body !== "string" && !(body instanceof ArrayBuffer);
  const res = await fetch(path, {
    method,
    headers: { Authorization: `Bearer ${t}`, ...(isJson ? { "Content-Type": "application/json" } : {}), ...headers },
    body: body === undefined ? undefined : isJson ? JSON.stringify(body) : (body as BodyInit),
  });
  if (res.status === 401) throw new ApiError(401, "Your session ended. Sign in again.");
  if (!res.ok) throw await readError(res);
  return res;
}

const json = async <T>(p: Promise<Response>): Promise<T> => (await p).json() as Promise<T>;
const seg = (path: string) => path.split("/").map(encodeURIComponent).join("/");
const enc = encodeURIComponent;
const none = (p: Promise<Response>) => p.then(() => undefined);

export const me = () => json<Me>(send("GET", "/api/v1/me"));

// Account console (an organization)
export const createAccount = (org: string) => none(send("POST", `/api/v1/orgs/${enc(org)}/account`, {}));
export const admins = async (org: string) => (await json<{ items: string[] }>(send("GET", `/api/v1/orgs/${enc(org)}/admins`))).items;
export const addAdmin = (org: string, principal: string) => none(send("PUT", `/api/v1/orgs/${enc(org)}/admins`, { principal }));
export const removeAdmin = (org: string, principal: string) => none(send("DELETE", `/api/v1/orgs/${enc(org)}/admins/${enc(principal)}`));
export const workspaces = async (org: string) => (await json<{ items: Workspace[] }>(send("GET", `/api/v1/orgs/${enc(org)}/workspaces`))).items;
export const createWorkspace = (org: string, name: string) => json<Workspace>(send("POST", `/api/v1/orgs/${enc(org)}/workspaces`, { name }));
export const audit = async (org: string, limit = 200) => (await json<{ items: AuditEntry[] }>(send("GET", `/api/v1/orgs/${enc(org)}/audit?limit=${limit}`))).items;
export const verify = (org: string) => json<Verdict>(send("GET", `/api/v1/orgs/${enc(org)}/audit/verify`));
export const erase = (org: string, subject: string) => json<{ task_id: string }>(send("POST", `/api/v1/orgs/${enc(org)}/erase`, { subject }));
export const task = (org: string, id: string) => json<Task>(send("GET", `/api/v1/orgs/${enc(org)}/tasks/${enc(id)}`));

// A workspace
const W = (ws: string) => `/api/v1/workspaces/${enc(ws)}`;
export const workspace = (ws: string) => json<Workspace>(send("GET", W(ws)));
export const renameWorkspace = (ws: string, name: string) => json<Workspace>(send("PUT", W(ws), { name }));
export const deleteWorkspace = (ws: string) => none(send("DELETE", W(ws)));
export const members = async (ws: string) => (await json<{ items: Member[] }>(send("GET", `${W(ws)}/members`))).items;
export const assignMember = (ws: string, principal: string, role: "admin" | "user") => none(send("PUT", `${W(ws)}/members`, { principal, role }));
export const removeMember = (ws: string, principal: string) => none(send("DELETE", `${W(ws)}/members/${enc(principal)}`));
export const entitlements = (ws: string) => json<{ items: { principal: string; entitlement: string }[]; available: string[] }>(send("GET", `${W(ws)}/entitlements`));
export const entitle = (ws: string, principal: string, entitlement: string) => none(send("PUT", `${W(ws)}/entitlements`, { principal, entitlement }));
export const unentitle = (ws: string, principal: string, entitlement: string) => none(send("DELETE", `${W(ws)}/entitlements/${enc(principal)}/${enc(entitlement)}`));
export const boundCatalogs = async (ws: string) => (await json<{ items: string[] }>(send("GET", `${W(ws)}/catalogs`))).items;
export const bindCatalog = (ws: string, catalog: string) => none(send("PUT", `${W(ws)}/catalogs/${enc(catalog)}`, {}));
export const unbindCatalog = (ws: string, catalog: string) => none(send("DELETE", `${W(ws)}/catalogs/${enc(catalog)}`));

// The catalog, in a workspace
export const list = async (ws: string, parent = "") => (await json<{ items: Securable[] }>(send("GET", `${W(ws)}/securables?parent_id=${enc(parent)}`))).items;
export const get = (ws: string, id: string) => json<Securable>(send("GET", `${W(ws)}/securables/${enc(id)}`));
export const create = (ws: string, b: { kind: Kind; parent_id: string; name: string; classification?: string; consistency?: string }) => json<Securable>(send("POST", `${W(ws)}/securables`, b));
export const drop = (ws: string, id: string) => json<{ task_id: string }>(send("DELETE", `${W(ws)}/securables/${enc(id)}`));
export const transfer = (ws: string, id: string, owner: string) => none(send("PUT", `${W(ws)}/securables/${enc(id)}/owner`, { owner }));
export const grants = async (ws: string, id: string) => (await json<{ items: Held[] }>(send("GET", `${W(ws)}/securables/${enc(id)}/grants`))).items;
export const assign = (ws: string, id: string, principal: string, role: Role) => none(send("PUT", `${W(ws)}/securables/${enc(id)}/grants`, { principal, role }));
export const revoke = (ws: string, id: string, principal: string) => none(send("DELETE", `${W(ws)}/securables/${enc(id)}/grants/${enc(principal)}`));

/** What holds files: a volume (any files) or a table (CSV or Parquet files a query reads as rows). */
export const files = async (ws: string, id: string) => (await json<{ items: FileEntry[] }>(send("GET", `${W(ws)}/volumes/${enc(id)}/files`))).items;
export const upload = (ws: string, id: string, path: string, file: Blob) => none(send("PUT", `${W(ws)}/volumes/${enc(id)}/files/${seg(path)}`, file, { "Content-Type": "application/octet-stream" }));
export const download = async (ws: string, id: string, path: string) => (await send("GET", `${W(ws)}/volumes/${enc(id)}/files/${seg(path)}`)).blob();
export const removeFile = (ws: string, id: string, path: string) => none(send("DELETE", `${W(ws)}/volumes/${enc(id)}/files/${seg(path)}`));

// Tables are Iceberg tables
export interface Column {
  name: string;
  type: string;
  required?: boolean;
}
export interface TableInfo {
  format: "iceberg";
  location: string;
  columns: { id: number; name: string; type: string | object; required: boolean }[];
  snapshots: { id: string; at_ms: number; operation?: string; records?: string; files?: string }[];
  current_snapshot: string;
  properties: Record<string, string>;
}
export const createTable = (ws: string, b: { parent_id: string; name: string; columns: Column[]; classification?: string }) => json<{ id: string }>(send("POST", `${W(ws)}/tables`, b));
export const tableInfo = (ws: string, id: string) => json<TableInfo>(send("GET", `${W(ws)}/tables/${enc(id)}`));

// Compute
export interface Compute {
  id: string;
  name: string;
  /** `agent`: a machine that connects out. `serverless`: a provider's endpoint, started when work arrives. */
  kind: "agent" | "serverless";
  online: boolean;
  max_memory_mb: number;
  max_cpu_millis: number;
  last_seen_ms: number;
  created_ms: number;
  /** Only in the answer to attaching it or giving it a new secret, and only once. */
  secret?: string;
}
export type RunState = "queued" | "running" | "succeeded" | "failed" | "cancelled" | "timed_out";
export interface Run {
  id: string;
  kind: "sql" | "job";
  state: RunState;
  title: string;
  requested_by: string;
  on_behalf_of: string;
  target_id: string;
  created_ms: number;
  started_ms: number;
  finished_ms: number;
  error: string;
  exit_code: number;
  cancelling: boolean;
  // Only on the one run, not in a list:
  spec?: { sql?: string; script?: string; tables?: { id: string; name: string }[]; inputs?: { id: string; kind: string; name: string }[]; output?: string; memory_mb?: number; cpu_millis?: number; timeout_s?: number };
  stdout?: string;
  stderr?: string;
  result?: { columns: { name: string; type: string }[]; rows: unknown[][]; truncated: boolean } | null;
}
export const isRunning = (r: Pick<Run, "state">) => r.state === "queued" || r.state === "running";

export const computeTargets = async (ws: string) => (await json<{ items: Compute[] }>(send("GET", `${W(ws)}/compute/targets`))).items;
export const runStatement = (ws: string, sql: string, limit: number, waitS = 30) => json<Run>(send("POST", `${W(ws)}/sql/statements`, { sql, limit, wait_s: waitS }));
export const submitJob = (ws: string, b: { title: string; script: string; inputs: string[]; output: string; timeout_s?: number }) => json<Run>(send("POST", `${W(ws)}/runs`, b));
export const runs = async (ws: string, kind?: "sql" | "job", limit = 50) => (await json<{ items: Run[] }>(send("GET", `${W(ws)}/runs?limit=${limit}${kind ? `&kind=${kind}` : ""}`))).items;
export const run = (ws: string, id: string) => json<Run>(send("GET", `${W(ws)}/runs/${enc(id)}`));
export const cancelRun = (ws: string, id: string) => json<Run>(send("POST", `${W(ws)}/runs/${enc(id)}/cancel`, {}));

// Compute, in the account console
export const attachedCompute = async (org: string) => (await json<{ items: Compute[] }>(send("GET", `/api/v1/orgs/${enc(org)}/compute/targets`))).items;
export const attachCompute = (org: string, b: { name: string; kind?: "agent" | "serverless"; endpoint?: string; max_memory_mb: number; max_cpu_millis: number }) => json<Compute>(send("POST", `/api/v1/orgs/${enc(org)}/compute/targets`, b));
export const newComputeSecret = (org: string, id: string) => json<Compute>(send("POST", `/api/v1/orgs/${enc(org)}/compute/targets/${enc(id)}/secret`, {}));
export const detachCompute = (org: string, id: string) => none(send("DELETE", `/api/v1/orgs/${enc(org)}/compute/targets/${enc(id)}`));
