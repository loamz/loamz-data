import { Bot, User, Users, type LucideIcon } from "lucide-react";

export type PrincipalType = "user" | "group" | "sp";

export function parsePrincipal(p: string): { type: PrincipalType; id: string } {
  const i = p.indexOf(":");
  const t = p.slice(0, i);
  return { type: t === "group" ? "group" : t === "sp" ? "sp" : "user", id: p.slice(i + 1) };
}

export const principalIcon: Record<PrincipalType, LucideIcon> = { user: User, group: Users, sp: Bot };
export const typeLabel: Record<PrincipalType, string> = { user: "Person", group: "Group", sp: "Service principal" };

/** What to show for a principal: groups by path, people and services by the start of a long id. */
export function principalLabel(p: string): string {
  const { type, id } = parsePrincipal(p);
  if (type === "group") return id;
  return /^[0-9a-f]{8}-[0-9a-f-]{27}$/.test(id) ? id.slice(0, 8) : id;
}

export const toPrincipal = (type: PrincipalType, id: string) => `${type}:${id.trim()}`;
