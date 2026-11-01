import type { Kind } from "./api";

/** What this person opened lately in a workspace, newest first. It lives in this browser only: a convenience, never the record of anything. */
export interface Recent {
  id: string;
  name: string;
  kind: Kind;
  at: number;
}

const LIMIT = 12;
const key = (ws: string) => `loamz.recents.${ws}`;

export function recents(ws: string): Recent[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(key(ws)) ?? "[]");
    return Array.isArray(raw) ? raw.filter((r): r is Recent => typeof r === "object" && r !== null && typeof (r as Recent).id === "string") : [];
  } catch {
    return [];
  }
}

export function remember(ws: string, item: { id: string; name: string; kind: Kind }): void {
  try {
    const next = [{ ...item, at: Date.now() }, ...recents(ws).filter((r) => r.id !== item.id)].slice(0, LIMIT);
    localStorage.setItem(key(ws), JSON.stringify(next));
  } catch {
    /* without storage there are no recents */
  }
}
