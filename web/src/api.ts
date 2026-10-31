// The Data API, one function per call. Errors are RFC 9457 problem documents; they become an `ApiError` carrying the
// server's own words, which are written to be shown to a person.

import { getToken, signOut } from "./auth";

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

export interface Me {
  subject: string;
  kind: "person" | "service";
  tenant: string;
  agent: string | null;
  tenant_owner: boolean;
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
  const token = getToken();
  if (!token) throw new ApiError(401, "Sign in to continue.");
  const isJson = body !== undefined && !(body instanceof Blob) && typeof body !== "string" && !(body instanceof ArrayBuffer);
  const res = await fetch(path, {
    method,
    headers: { Authorization: `Bearer ${token}`, ...(isJson ? { "Content-Type": "application/json" } : {}), ...headers },
    body: body === undefined ? undefined : isJson ? JSON.stringify(body) : (body as BodyInit),
  });
  if (res.status === 401) {
    signOut();
    throw new ApiError(401, "Your session ended. Sign in again.");
  }
  if (!res.ok) throw await readError(res);
  return res;
}

const json = async <T>(p: Promise<Response>): Promise<T> => (await p).json() as Promise<T>;
const seg = (path: string) => path.split("/").map(encodeURIComponent).join("/");

export const me = () => json<Me>(send("GET", "/api/v1/me"));
export const list = async (parent = "") => (await json<{ items: Securable[] }>(send("GET", `/api/v1/securables?parent_id=${encodeURIComponent(parent)}`))).items;
export const get = (id: string) => json<Securable>(send("GET", `/api/v1/securables/${id}`));
export const create = (b: { kind: Kind; parent_id: string; name: string; classification?: string; consistency?: string }) => json<Securable>(send("POST", "/api/v1/securables", b));
export const drop = (id: string) => json<{ task_id: string }>(send("DELETE", `/api/v1/securables/${id}`));

export const grants = async (id: string) => (await json<{ items: Held[] }>(send("GET", `/api/v1/securables/${id}/grants`))).items;
export const assign = (id: string, principal: string, role: Role) => send("PUT", `/api/v1/securables/${id}/grants`, { principal, role }).then(() => undefined);
export const revoke = (id: string, principal: string) => send("DELETE", `/api/v1/securables/${id}/grants/${encodeURIComponent(principal)}`).then(() => undefined);

export const files = async (volume: string) => (await json<{ items: FileEntry[] }>(send("GET", `/api/v1/volumes/${volume}/files`))).items;
export const upload = (volume: string, path: string, file: Blob) => send("PUT", `/api/v1/volumes/${volume}/files/${seg(path)}`, file, { "Content-Type": "application/octet-stream" }).then(() => undefined);
export const download = async (volume: string, path: string) => (await send("GET", `/api/v1/volumes/${volume}/files/${seg(path)}`)).blob();
export const removeFile = (volume: string, path: string) => send("DELETE", `/api/v1/volumes/${volume}/files/${seg(path)}`).then(() => undefined);

export const audit = async (limit = 200) => (await json<{ items: AuditEntry[] }>(send("GET", `/api/v1/audit?limit=${limit}`))).items;
export const verify = () => json<Verdict>(send("GET", "/api/v1/audit/verify"));
export const erase = (subject: string) => json<{ task_id: string }>(send("POST", "/api/v1/erase", { subject }));
export const task = (id: string) => json<Task>(send("GET", `/api/v1/tasks/${id}`));
