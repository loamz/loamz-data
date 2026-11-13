import { useEffect, useState } from "react";
import { Button, toast } from "@/design";
import * as api from "@/api";
import { useLoad } from "@/hooks";

/** Which of the workspace's compute may read a catalog. Nothing ticked means any of it. Whoever manages the catalog decides. */
export function CatalogCompute({ ws, id }: { ws: string; id: string }) {
  const policy = useLoad(() => api.catalogCompute(ws, id), `${ws}/${id}`);
  const targets = useLoad(() => api.computeTargets(ws), ws);
  const [picked, setPicked] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  useEffect(() => setPicked(policy.data ?? []), [policy.data]);
  const all = targets.data ?? [];
  const changed = JSON.stringify([...picked].sort()) !== JSON.stringify([...(policy.data ?? [])].sort());
  return (
    <div className="flex max-w-lg flex-col gap-3">
      <p className="text-sm text-muted">Which compute may read this catalog. With none ticked, any compute of this workspace may.</p>
      <div className="divide-y divide-border rounded-xl border border-border bg-background/40">
        {all.length === 0 && <p className="px-4 py-3 text-sm text-muted">This workspace has no compute yet.</p>}
        {all.map((c) => (
          <label key={c.id} className="flex cursor-pointer items-center gap-3 px-4 py-3 text-sm">
            <input type="checkbox" checked={picked.includes(c.id)} onChange={(e) => setPicked(e.target.checked ? [...picked, c.id] : picked.filter((p) => p !== c.id))} />
            <span className="font-medium">{c.name}</span>
            <span className="text-xs text-muted">{c.kind === "serverless" ? "Serverless" : "Machine"}</span>
          </label>
        ))}
      </div>
      <div>
        <Button
          variant="primary"
          disabled={!changed || busy}
          onClick={async () => {
            setBusy(true);
            try {
              await api.setCatalogCompute(ws, id, picked);
              policy.reload();
              toast.success("Saved");
            } catch (err) {
              toast.error("Could not save it", (err as Error).message);
            } finally {
              setBusy(false);
            }
          }}
        >
          Save
        </Button>
      </div>
    </div>
  );
}
