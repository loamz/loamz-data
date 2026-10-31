import { ShieldCheck, UserMinus } from "lucide-react";
import { useState } from "react";
import { Badge, Button, PageEmpty, Select, confirmAction, toast } from "@/design";
import * as api from "@/api";
import { useLoad } from "@/hooks";
import { Principal } from "./Principal";
import { PrincipalField } from "./PrincipalField";

const roles = [
  { value: "reader", label: "Reader" },
  { value: "contributor", label: "Contributor" },
  { value: "owner", label: "Owner" },
];
const meaning: Record<string, string> = {
  reader: "Reader: can look and read (BROWSE, SELECT, READ VOLUME).",
  contributor: "Contributor: can also change, write and create things inside.",
  owner: "Owner: can also manage access and drop (MANAGE).",
};

/** What a person, group or service principal holds on one thing (or on the whole metastore when `id` is "metastore"). Only people who manage it may see or change this. */
export function AccessList({ ws, id }: { ws: string; id: string }) {
  const held = useLoad(() => api.grants(ws, id), `${ws}/${id}`);
  const [principal, setPrincipal] = useState("");
  const [role, setRole] = useState("reader");
  const [busy, setBusy] = useState(false);

  if (held.error && held.error.status < 500 && !held.data) {
    return (
      <PageEmpty icon={ShieldCheck} title="Only managers can see who has access">
        Ask an owner of this, or an account admin, to give you the Owner role.
      </PageEmpty>
    );
  }
  const items = held.data ?? [];

  return (
    <div className="space-y-4">
      <form
        className="flex flex-wrap items-center gap-2"
        onSubmit={async (e) => {
          e.preventDefault();
          if (!principal) return;
          setBusy(true);
          try {
            await api.assign(ws, id, principal, role as api.Role);
            toast.success("Access given");
            held.reload();
          } catch (err) {
            toast.error("Could not give access", (err as Error).message);
          } finally {
            setBusy(false);
          }
        }}
      >
        <PrincipalField onChange={setPrincipal} className="min-w-72" />
        <Select className="w-40" aria-label="Role" value={role} onValueChange={setRole} options={roles} />
        <Button variant="primary" type="submit" disabled={busy || !principal}>
          Give access
        </Button>
      </form>
      <p className="px-1 text-xs text-muted">{meaning[role]}</p>

      <div className="divide-y divide-border rounded-xl border border-border bg-background/40">
        {items.length === 0 && !held.loading && <p className="px-4 py-6 text-center text-xs text-muted">Nobody has been given access here directly. Access given on a place above it still reaches here.</p>}
        {items.map((h) => (
          <div key={h.principal} className="flex items-center gap-3 px-4 py-2.5">
            <div className="min-w-0 flex-1">
              <Principal id={h.principal} />
              <div className="mt-1 flex flex-wrap gap-1">
                {h.privileges.map((p) => (
                  <Badge key={p} tone={p === "MANAGE" || p === "ALL_PRIVILEGES" ? "accent" : "neutral"}>
                    {p.replace(/_/g, " ")}
                  </Badge>
                ))}
              </div>
            </div>
            <Button
              variant="danger"
              size="icon-sm"
              aria-label={`Remove access for ${h.principal}`}
              onClick={async () => {
                if (!(await confirmAction({ title: "Remove this access?", description: "They lose everything they were given directly here.", confirmLabel: "Remove", danger: true }))) return;
                try {
                  await api.revoke(ws, id, h.principal);
                  held.reload();
                } catch (err) {
                  toast.error("Could not remove access", (err as Error).message);
                }
              }}
            >
              <UserMinus />
            </Button>
          </div>
        ))}
      </div>
    </div>
  );
}
