// The address is the screen. Real paths, so a link can be shared and the server can serve the page at any of them. There are
// two sites (see site.ts), each with its own paths:
//   account site    /                        Home
//                   /account/<org>[/<tab>]   the account console of an organization (tabs: workspaces, compute, people, admins, activity, erase)
//   workspace site  /                        the workspace's overview: what you can do in it
//                   /catalog[/<id>]          its Catalog Explorer
//                   /sql                     the SQL editor
//                   /queries                 saved queries
//                   /history                 every statement and job that ran, who ran it and how long it took
//                   /jobs                    jobs that ran, and the button to start one
//                   /runs/<id>               one run: how it went and what it printed
//                   /compute                 the compute work runs on
//                   /settings[/<tab>]        its members, service principals, storage and entitlements (tabs: members, principals, storage, entitlements, workspace)

import type { MouseEvent } from "react";

export type SettingsTab = "members" | "principals" | "storage" | "entitlements" | "workspace";
export type AccountTab = "workspaces" | "compute" | "people" | "admins" | "activity" | "erase";

export type Route =
  | { screen: "home" }
  | { screen: "workspace"; ws: string }
  | { screen: "sql"; ws: string }
  | { screen: "queries"; ws: string }
  | { screen: "history"; ws: string }
  | { screen: "jobs"; ws: string }
  | { screen: "run"; ws: string; id: string }
  | { screen: "compute"; ws: string }
  | { screen: "catalog"; ws: string; id: string | null }
  | { screen: "settings"; ws: string; tab: SettingsTab }
  | { screen: "account"; org: string; tab: AccountTab };

const SETTINGS_TABS: SettingsTab[] = ["members", "principals", "storage", "entitlements", "workspace"];
const TABS: AccountTab[] = ["workspaces", "compute", "people", "admins", "activity", "erase"];
const WS = /^[0-9a-f-]{8,64}$/;

export function parseRoute(path: string, kind: "account" | "workspace", ws = ""): Route {
  const parts = path.split("/").filter(Boolean).map(decodeURIComponent);
  if (kind === "workspace") {
    if (parts[0] === "settings") return { screen: "settings", ws, tab: SETTINGS_TABS.find((t) => t === parts[1]) ?? "members" };
    if (parts[0] === "sql") return { screen: "sql", ws };
    if (parts[0] === "queries") return { screen: "queries", ws };
    if (parts[0] === "history") return { screen: "history", ws };
    if (parts[0] === "jobs") return { screen: "jobs", ws };
    if (parts[0] === "runs" && parts[1]) return { screen: "run", ws, id: parts[1] };
    if (parts[0] === "compute") return { screen: "compute", ws };
    if (parts[0] === "catalog") return { screen: "catalog", ws, id: parts[1] && WS.test(parts[1]) ? parts[1] : null };
    return { screen: "workspace", ws };
  }
  if (parts[0] === "account" && parts[1]) {
    const tab = TABS.find((t) => t === parts[2]);
    return { screen: "account", org: parts[1], tab: tab ?? "workspaces" };
  }
  return { screen: "home" };
}

export function hrefOf(r: Route): string {
  switch (r.screen) {
    case "home":
      return "/";
    case "workspace":
      return "/";
    case "catalog":
      return `/catalog${r.id ? `/${r.id}` : ""}`;
    case "settings":
      return r.tab === "members" ? "/settings" : `/settings/${r.tab}`;
    case "sql":
      return "/sql";
    case "queries":
      return "/queries";
    case "history":
      return "/history";
    case "jobs":
      return "/jobs";
    case "run":
      return `/runs/${encodeURIComponent(r.id)}`;
    case "compute":
      return "/compute";
    case "account":
      return `/account/${encodeURIComponent(r.org)}/${r.tab}`;
  }
}

/** Moves to a screen without reloading the page. */
export function go(r: Route | string): void {
  const to = typeof r === "string" ? r : hrefOf(r);
  if (to !== location.pathname) {
    history.pushState(null, "", to);
    dispatchEvent(new PopStateEvent("popstate"));
  }
}

/** The click handler of a link: it moves without reloading, and still opens in a new tab when asked to. */
export const navigate = (to: Route) => (e: MouseEvent<HTMLElement>) => {
  if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
  e.preventDefault();
  go(to);
};
