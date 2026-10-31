import { UserMinus } from "lucide-react";
import { useState } from "react";
import { Badge, Button, Input, PageEmpty, Select, confirmAction, toast } from "@/design";
import * as api from "@/api";
import { useLoad } from "@/hooks";
import { shortId } from "@/kinds";
import { ShieldCheck } from "lucide-react";

const roles = [
  { value: "reader", label: "Reader" },
  { value: "contributor", label: "Contributor" },
  { value: "owner", label: "Owner" },
];
const meaning: Record<string, string> = { reader: "Reader: can look and read.", contributor: "Contributor: can also write and create.", owner: "Owner: can also manage access and drop." };

/** Who holds what on one thing (or on the whole tenant when `id` is "root"). Only people who manage it may see or change this. */
export function AccessList({ id }: { id: string }) {
  const held = useLoad(() => api.grants(id), id);
  const [principal, setPrincipal] = useState("");
  const [role, setRole] = useState("reader");
  const [busy, setBusy] = useState(false);

  if (held.error && held.error.status < 500 && !held.data) {
    return (
      <PageEmpty icon={ShieldCheck} title="Only managers can see who has access">
        Ask an owner of this to give you the Owner role.
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
          if (!principal.trim()) return;
          setBusy(true);
          try {
            await api.assign(id, principal.trim(), role as api.Role);
            toast.success("Access given");
            setPrincipal("");
            held.reload();
          } catch (err) {
            toast.error("Could not give access", (err as Error).message);
          } finally {
            setBusy(false);
          }
        }}
      >
        <Input className="min-w-56 flex-1" placeholder="Person or agent id" aria-label="Person or agent id" value={principal} onChange={(e) => setPrincipal(e.target.value)} spellCheck={false} />
        <Select className="w-40" aria-label="Role" value={role} onValueChange={setRole} options={roles} />
        <Button variant="primary" type="submit" disabled={busy || !principal.trim()}>
          Give access
        </Button>
      </form>
      <p className="px-1 text-xs text-muted">{meaning[role]}</p>

      <div className="divide-y divide-border rounded-xl border border-border bg-background/40">
        {items.length === 0 && !held.loading && <p className="px-4 py-6 text-center text-xs text-muted">Nobody has been given access here directly. Access given on a place above it still reaches here.</p>}
        {items.map((h) => (
          <div key={h.principal} className="flex items-center gap-3 px-4 py-2.5">
            <span className="min-w-0 flex-1 truncate font-mono text-xs text-foreground" title={h.principal}>
              {shortId(h.principal)}
            </span>
            <Badge tone={h.role === "owner" ? "accent" : "neutral"}>{h.role ?? h.privileges.join(", ")}</Badge>
            <Button
              variant="danger"
              size="icon-sm"
              aria-label={`Remove access for ${shortId(h.principal)}`}
              onClick={async () => {
                if (!(await confirmAction({ title: "Remove this access?", description: "They lose everything they were given directly here.", confirmLabel: "Remove", danger: true }))) return;
                try {
                  await api.revoke(id, h.principal);
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
