import { Database, Eye, Folder, FolderTree, Table2, type LucideIcon } from "lucide-react";
import type { Kind } from "./api";

export const kindIcon: Record<Kind, LucideIcon> = { catalog: Database, schema: FolderTree, table: Table2, volume: Folder, view: Eye };
export const kindLabel: Record<Kind, string> = { catalog: "Catalog", schema: "Schema", table: "Table", volume: "Volume", view: "View" };

/** What can be made inside a given kind of thing (or at the top, `null`). */
export function childKinds(parent: Kind | null): Kind[] {
  if (parent === null) return ["catalog"];
  if (parent === "catalog") return ["schema"];
  if (parent === "schema") return ["volume", "table", "view"];
  return [];
}

export const canContain = (k: Kind) => childKinds(k).length > 0;

/** Same rule as the server: lower-case letters, digits and underscores, starting with a letter, 63 at most. */
export const validName = (n: string) => /^[a-z][a-z0-9_]{0,62}$/.test(n);

/** A person id from the sign-in service is a long UUID; show the start of it, keep the whole in a tooltip. */
export const shortId = (id: string) => (/^[0-9a-f]{8}-[0-9a-f-]{27}$/.test(id) ? id.slice(0, 8) : id);
