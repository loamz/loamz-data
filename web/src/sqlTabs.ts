// The SQL editor's open tabs, kept in this browser for each workspace so closing the page loses nothing. They are drafts, not
// anything the server knows about: a statement becomes a record only when it is run.

import { go } from "./route";

export interface Tab {
  id: string;
  title: string;
  sql: string;
  /** Set when the tab holds a saved query: its id, and the SQL as last saved (so a change shows as unsaved). */
  saved?: { id: string; name: string; sql: string };
}
export interface Tabs {
  tabs: Tab[];
  active: string;
}

const key = (ws: string) => `loamz-data-sql:${ws}`;
let counter = 0;
const newId = () => `${Date.now().toString(36)}${(counter++).toString(36)}`;

export function blank(title = "Untitled", sql = ""): Tab {
  return { id: newId(), title, sql };
}

export function load(ws: string): Tabs {
  try {
    const saved = JSON.parse(localStorage.getItem(key(ws)) ?? "null") as Tabs | null;
    if (saved && Array.isArray(saved.tabs) && saved.tabs.length > 0 && saved.tabs.some((t) => t.id === saved.active)) return saved;
  } catch {
    /* unreadable or storage blocked: start fresh */
  }
  const first = blank("Query 1");
  return { tabs: [first], active: first.id };
}

export function save(ws: string, state: Tabs): void {
  try {
    localStorage.setItem(key(ws), JSON.stringify(state));
  } catch {
    /* storage blocked: the drafts last until the page closes */
  }
}

/** The tabs with a statement opened in a new one. */
export function withStatement(state: Tabs, sql: string, title: string): Tabs {
  const tab = blank(title, sql);
  return { tabs: [...state.tabs, tab], active: tab.id };
}

/** The tabs with a saved query open: the tab it already has, or a new one. */
export function withSaved(state: Tabs, q: { id: string; name: string; sql: string }): Tabs {
  const open = state.tabs.find((t) => t.saved?.id === q.id);
  if (open) return { ...state, active: open.id };
  const tab: Tab = { ...blank(q.name, q.sql), saved: { id: q.id, name: q.name, sql: q.sql } };
  return { tabs: [...state.tabs, tab], active: tab.id };
}

/** Opens a statement in a new tab of the workspace's SQL editor. */
export function openInEditor(ws: string, sql: string, title: string): void {
  save(ws, withStatement(load(ws), sql, title));
  go({ screen: "sql", ws });
}

/** Opens a saved query in its own tab (the one already open, if it is). */
export function openSaved(ws: string, q: { id: string; name: string; sql: string }): void {
  save(ws, withSaved(load(ws), q));
  go({ screen: "sql", ws });
}

/** The name of a tab from its first words, so a row of tabs says what is in each. */
export function titleOf(sql: string, fallback: string): string {
  const flat = sql.replace(/--.*$/gm, "").replace(/\s+/g, " ").trim();
  return flat ? (flat.length > 24 ? `${flat.slice(0, 24)}…` : flat) : fallback;
}
