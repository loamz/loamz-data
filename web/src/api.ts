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
  /** The installation's admin: creates organizations. */
  platform_admin: boolean;
}
/** Someone just added to an organization. The password is set only when they were made now: shown once, changed at first sign-in. */
export interface Invited {
  subject: string;
  email: string;
  temporary_password: string | null;
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
export const createOrganization = (alias: string, admin_email: string, admin_name: string) =>
  json<{ id: string; alias: string; admin: Invited }>(send("POST", "/api/v1/orgs", { alias, admin_email, admin_name }));
export const addPerson = (org: string, email: string, name: string) => json<Invited>(send("POST", `/api/v1/orgs/${enc(org)}/people`, { email, name }));
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
export interface ServicePrincipal {
  id: string;
  name: string;
  /** How grants and assignments name it: `sp:<id>`. */
  principal: string;
  created_by: string;
  created_ms: number;
}
export const servicePrincipals = async (ws: string) => (await json<{ items: ServicePrincipal[] }>(send("GET", `${W(ws)}/service-principals`))).items;
/** The token is in the answer once. */
export const createServicePrincipal = (ws: string, name: string) => json<ServicePrincipal & { token: string }>(send("POST", `${W(ws)}/service-principals`, { name }));
export const renewServicePrincipalToken = (ws: string, id: string) => json<{ token: string }>(send("POST", `${W(ws)}/service-principals/${enc(id)}/token`, {}));
export const deleteServicePrincipal = (ws: string, id: string) => none(send("DELETE", `${W(ws)}/service-principals/${enc(id)}`));
export interface StorageCredential {
  id: string;
  name: string;
  endpoint: string;
  region: string;
  /** References into the vault, never secrets. */
  key_id_ref: string;
  secret_ref: string;
  vending: { kind: "none" | "sts" | "r2"; role_arn?: string; token_ref?: string };
}
export interface StorageLocation {
  id: string;
  name: string;
  credential_id: string;
  bucket: string;
  prefix: string;
}
const S = (ws: string) => `/api/v1/workspaces/${enc(ws)}/storage`;
export const storageCredentials = async (ws: string) => (await json<{ items: StorageCredential[] }>(send("GET", `${S(ws)}/credentials`))).items;
export const createStorageCredential = (ws: string, body: Omit<StorageCredential, "id">) => json<StorageCredential>(send("POST", `${S(ws)}/credentials`, body));
export const deleteStorageCredential = (ws: string, id: string) => none(send("DELETE", `${S(ws)}/credentials/${enc(id)}`));
export const storageLocations = async (ws: string) => (await json<{ items: StorageLocation[] }>(send("GET", `${S(ws)}/locations`))).items;
export const createStorageLocation = (ws: string, body: Omit<StorageLocation, "id">) => json<StorageLocation>(send("POST", `${S(ws)}/locations`, body));
export const deleteStorageLocation = (ws: string, id: string) => none(send("DELETE", `${S(ws)}/locations/${enc(id)}`));
export type TableRules = { row_filter: string; masks: Record<string, string> };
export const tableRules = (ws: string, id: string) => json<TableRules>(send("GET", `${W(ws)}/securables/${enc(id)}/policy`));
export const setTableRules = (ws: string, id: string, rules: TableRules) => none(send("PUT", `${W(ws)}/securables/${enc(id)}/policy`, rules));
export const catalogCompute = async (ws: string, catalog: string) => (await json<{ compute: string[] }>(send("GET", `${W(ws)}/catalogs/${enc(catalog)}/compute`))).compute;
export const setCatalogCompute = (ws: string, catalog: string, compute: string[]) => none(send("PUT", `${W(ws)}/catalogs/${enc(catalog)}/compute`, { compute }));
export const workspace = (ws: string) => json<Workspace>(send("GET", W(ws)));
export const renameWorkspace = (ws: string, name: string) => json<Workspace>(send("PUT", W(ws), { name }));
export const deleteWorkspace = (ws: string) => none(send("DELETE", W(ws)));
export const members = async (ws: string) => (await json<{ items: Member[] }>(send("GET", `${W(ws)}/members`))).items;
export const assignMember = (ws: string, principal: string, role: "admin" | "user") => none(send("PUT", `${W(ws)}/members`, { principal, role }));
export const removeMember = (ws: string, principal: string) => none(send("DELETE", `${W(ws)}/members/${enc(principal)}`));
export const entitlements = (ws: string) => json<{ items: { principal: string; entitlement: string }[]; available: string[] }>(send("GET", `${W(ws)}/entitlements`));
export const entitle = (ws: string, principal: string, entitlement: string) => none(send("PUT", `${W(ws)}/entitlements`, { principal, entitlement }));
export const unentitle = (ws: string, principal: string, entitlement: string) => none(send("DELETE", `${W(ws)}/entitlements/${enc(principal)}/${enc(entitlement)}`));

// The catalog, in a workspace
export const list = async (ws: string, parent = "") => (await json<{ items: Securable[] }>(send("GET", `${W(ws)}/securables?parent_id=${enc(parent)}`))).items;
export const get = (ws: string, id: string) => json<Securable>(send("GET", `${W(ws)}/securables/${enc(id)}`));
export const create = (ws: string, b: { kind: Kind; parent_id: string; name: string; classification?: string; consistency?: string; location?: string }) => json<Securable>(send("POST", `${W(ws)}/securables`, b));
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
  /** About the whole table, from its current snapshot: no data read. */
  stats: { records: number | null; data_files: number | null; size_bytes: number | null; at_ms: number | null };
}
export const createTable = (ws: string, b: { parent_id: string; name: string; columns: Column[]; classification?: string }) => json<{ id: string }>(send("POST", `${W(ws)}/tables`, b));
export interface ColumnStat {
  id: number;
  name: string;
  values: number;
  nulls: number;
  min: string | null;
  max: string | null;
}
export interface TableStats {
  records: number;
  data_files: number;
  columns: ColumnStat[];
}
export const tableStats = (ws: string, id: string) => json<TableStats>(send("GET", `${W(ws)}/tables/${enc(id)}/stats`));
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
  /** When it last finished some work (0 for never): a serverless warehouse is running for a minute after this. */
  last_used_ms: number;
  /** Serverless compute that is starting up after someone started it. */
  starting: boolean;
  /** When a serverless warehouse was last stopped by hand (0 for never): it is stopped if that is after its last use. */
  stopped_ms: number;
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
  /** How long it waited before starting, and how long it ran once it had ended; -1 while not known yet. */
  queued_ms: number;
  run_ms: number;
  /** Run details omit output when access to an input cannot be verified. */
  result_access?: "available" | "restricted";
  // Only on the one run, not in a list:
  spec?: { sql?: string; script?: string; tables?: { id: string; name: string }[]; inputs?: { id: string; kind: string; name: string }[]; output?: string; memory_mb?: number; cpu_millis?: number; timeout_s?: number };
  stdout?: string;
  stderr?: string;
  result?: { columns: { name: string; type: string }[]; rows: unknown[][]; truncated: boolean } | null;
}
export const isRunning = (r: Pick<Run, "state">) => r.state === "queued" || r.state === "running";

export const computeTargets = async (ws: string) => (await json<{ items: Compute[] }>(send("GET", `${W(ws)}/compute/targets`))).items;
export const runStatement = (ws: string, sql: string, limit: number, waitS = 30, target?: string) => json<Run>(send("POST", `${W(ws)}/sql/statements`, { sql, limit, wait_s: waitS, ...(target ? { target } : {}) }));
export const submitJob = (ws: string, b: { title: string; script: string; inputs: string[]; output: string; timeout_s?: number }) => json<Run>(send("POST", `${W(ws)}/runs`, b));
export interface ImportFileRequest {
  target: string;
  volume: string;
  path: string;
  mode?: "create" | "append";
  format?: "csv" | "parquet";
  target_compute?: string;
}
export const importFile = (ws: string, b: ImportFileRequest) => json<Run>(send("POST", `${W(ws)}/imports`, b));
export interface RunFilter {
  kind?: "sql" | "job";
  /** A person's subject (an admin's narrowing). */
  by?: string;
  target?: string;
  state?: RunState;
  since_ms?: number;
  /** The `created_ms` of the oldest run of the page before. */
  before_ms?: number;
  limit?: number;
}
export const runs = async (ws: string, f: RunFilter = {}) => {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(f)) if (v !== undefined && v !== "") q.set(k, String(v));
  return (await json<{ items: Run[] }>(send("GET", `${W(ws)}/runs?${q.toString()}`))).items;
};
export const run = (ws: string, id: string) => json<Run>(send("GET", `${W(ws)}/runs/${enc(id)}`));
export const cancelRun = (ws: string, id: string) => json<Run>(send("POST", `${W(ws)}/runs/${enc(id)}/cancel`, {}));

// Compute, in the account console
export interface Provider {
  id: "serverless" | "machine";
  name: string;
  description: string;
  available: boolean;
  max_memory_mb: number;
  max_cpu_millis: number;
  idle_stop_s: number;
}
export const providers = async (ws: string) => (await json<{ items: Provider[] }>(send("GET", `${W(ws)}/compute/providers`))).items;
export const attachCompute = (ws: string, b: { name: string; kind?: "agent" | "serverless"; endpoint?: string; max_memory_mb: number; max_cpu_millis: number }) => json<Compute>(send("POST", `${W(ws)}/compute/targets`, b));
export const newComputeSecret = (ws: string, id: string) => json<Compute>(send("POST", `${W(ws)}/compute/targets/${enc(id)}/secret`, {}));
export const startCompute = (ws: string, id: string) => none(send("POST", `${W(ws)}/compute/targets/${enc(id)}/start`, {}));
export const stopCompute = (ws: string, id: string) => none(send("POST", `${W(ws)}/compute/targets/${enc(id)}/stop`, {}));
export const detachCompute = (ws: string, id: string) => none(send("DELETE", `${W(ws)}/compute/targets/${enc(id)}`));

// Saved queries
export interface SavedQuery {
  id: string;
  owner: string;
  name: string;
  sql: string;
  /** The compute it last ran on; empty for none. */
  target_id: string;
  created_ms: number;
  updated_ms: number;
}
export interface QueryDraft {
  name: string;
  sql: string;
  target_id?: string;
}
export const savedQueries = async (ws: string) => (await json<{ items: SavedQuery[] }>(send("GET", `${W(ws)}/queries`))).items;
export const savedQuery = (ws: string, id: string) => json<SavedQuery>(send("GET", `${W(ws)}/queries/${enc(id)}`));
export const saveQuery = (ws: string, d: QueryDraft) => json<SavedQuery>(send("POST", `${W(ws)}/queries`, d));
export const changeQuery = (ws: string, id: string, d: QueryDraft) => json<SavedQuery>(send("PUT", `${W(ws)}/queries/${enc(id)}`, d));
export const deleteQuery = (ws: string, id: string) => none(send("DELETE", `${W(ws)}/queries/${enc(id)}`));
