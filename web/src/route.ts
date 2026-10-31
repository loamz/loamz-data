// The address is the screen: #/catalog, #/catalog/<id>, #/activity, #/admin. No router library for four places.

export type Route = { screen: "catalog"; id: string | null } | { screen: "activity" } | { screen: "admin" };

export function parseRoute(hash: string): Route {
  const [first = "", second = ""] = hash.replace(/^#\/?/, "").split("/");
  if (first === "activity") return { screen: "activity" };
  if (first === "admin") return { screen: "admin" };
  return { screen: "catalog", id: first === "catalog" && /^[0-9a-f-]{8,64}$/.test(second) ? second : null };
}

export function hrefOf(r: Route): string {
  return r.screen === "catalog" ? (r.id ? `#/catalog/${r.id}` : "#/catalog") : `#/${r.screen}`;
}
