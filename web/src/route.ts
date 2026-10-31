// The address is the screen. Real paths, so a link can be shared and the server can serve the page at any of them. There are
// two sites (see site.ts), each with its own paths:
//   account site    /                        Home
//                   /account/<org>[/<tab>]   the account console of an organization (tabs: workspaces, admins, activity, erase)
//   workspace site  /                        the workspace's overview: what you can do in it
//                   /catalog[/<id>]          its Catalog Explorer
//                   /settings                its members, entitlements, bound catalogs

export type AccountTab = "workspaces" | "admins" | "activity" | "erase";

export type Route =
  | { screen: "home" }
  | { screen: "workspace"; ws: string }
  | { screen: "catalog"; ws: string; id: string | null }
  | { screen: "settings"; ws: string }
  | { screen: "account"; org: string; tab: AccountTab };

const TABS: AccountTab[] = ["workspaces", "admins", "activity", "erase"];
const WS = /^[0-9a-f-]{8,64}$/;

export function parseRoute(path: string, kind: "account" | "workspace", ws = ""): Route {
  const parts = path.split("/").filter(Boolean).map(decodeURIComponent);
  if (kind === "workspace") {
    if (parts[0] === "settings") return { screen: "settings", ws };
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
      return "/settings";
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
