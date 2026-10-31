// Which website this page is. The account site (`data.loamz.com`) is where you sign in and choose a workspace; every workspace is its
// own site (`w-<id>.data.loamz.com`), and nothing inside one links back out except signing out. The site is read from the address,
// so the same build serves both.

export type Site = { kind: "account"; base: string } | { kind: "workspace"; ws: string; base: string };

const HOST = /^w-([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\.(.+)$/i;

/** The site for an address (`host` includes the port). */
export function siteOf(host: string): Site {
  const m = HOST.exec(host);
  return m ? { kind: "workspace", ws: (m[1] ?? "").toLowerCase(), base: m[2] ?? "" } : { kind: "account", base: host };
}

/** Where the sign-in code must go back to when `state` names a workspace site of this account site; null for anything else. */
export function relayTarget(state: string, protocol: string, base: string): string | null {
  const dot = state.lastIndexOf(".");
  if (dot < 1) return null;
  try {
    const origin = atob(state.slice(dot + 1).replace(/-/g, "+").replace(/_/g, "/"));
    const url = new URL(origin);
    const s = siteOf(url.host);
    return url.protocol === protocol && url.origin === origin && s.kind === "workspace" && s.base === base ? origin : null;
  } catch {
    return null;
  }
}

export const site: Site = siteOf(typeof location === "undefined" ? "localhost" : location.host);
export const centralOrigin = typeof location === "undefined" ? "" : `${location.protocol}//${site.base}`;
export const workspaceOrigin = (id: string) => `${location.protocol}//w-${id}.${site.base}`;

/** Opens a workspace's own site. */
export function openWorkspace(id: string, path = ""): void {
  location.assign(workspaceOrigin(id) + path);
}
