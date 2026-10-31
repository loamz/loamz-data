// The address is the screen. Real paths, so a link can be shared and the server can serve the page at any of them:
//   /                        Home
//   /w/<ws>/catalog[/<id>]   a workspace's Catalog Explorer
//   /w/<ws>/settings         the workspace's members, entitlements, bound catalogs
//   /account/<org>[/<tab>]   the account console of an organization (tabs: workspaces, admins, activity, erase)

export type AccountTab = "workspaces" | "admins" | "activity" | "erase";

export type Route =
  | { screen: "home" }
  | { screen: "catalog"; ws: string; id: string | null }
  | { screen: "settings"; ws: string }
  | { screen: "account"; org: string; tab: AccountTab };

const TABS: AccountTab[] = ["workspaces", "admins", "activity", "erase"];
const WS = /^[0-9a-f-]{8,64}$/;

export function parseRoute(path: string): Route {
  const parts = path.split("/").filter(Boolean).map(decodeURIComponent);
  if (parts[0] === "w" && parts[1] && WS.test(parts[1])) {
    if (parts[2] === "settings") return { screen: "settings", ws: parts[1] };
    return { screen: "catalog", ws: parts[1], id: parts[2] === "catalog" && parts[3] && WS.test(parts[3]) ? parts[3] : null };
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
    case "catalog":
      return `/w/${r.ws}/catalog${r.id ? `/${r.id}` : ""}`;
    case "settings":
      return `/w/${r.ws}/settings`;
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
