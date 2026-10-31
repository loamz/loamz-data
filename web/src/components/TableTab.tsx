import { Copy, Database } from "lucide-react";
import { Badge, Button, PageEmpty, toast } from "@/design";
import * as api from "@/api";
import { useLoad } from "@/hooks";
import { when } from "@/format";
import { centralOrigin } from "@/site";

const typeText = (t: string | object) => (typeof t === "string" ? t : (t as { type?: string }).type ?? "nested");

/** What a table is: its columns and its history. The data is read with SQL, or by any Iceberg client through this workspace's catalog. */
export function TableTab({ ws, id, catalog }: { ws: string; id: string; catalog: string }) {
  const info = useLoad(() => api.tableInfo(ws, id), `${ws}/${id}`);
  if (info.error && !info.data) {
    return (
      <PageEmpty icon={Database} title="You can't open this table">
        {info.error.message}
      </PageEmpty>
    );
  }
  const t = info.data;
  if (!t) return <p className="text-xs text-muted">Loading…</p>;
  const connect = `INSTALL iceberg; LOAD iceberg;
ATTACH '${catalog}' AS ${catalog} (TYPE iceberg, ENDPOINT '${centralOrigin}/api/v1/workspaces/${ws}/iceberg', TOKEN '<your access token>');`;
  return (
    <div className="space-y-6">
      <section>
        <h3 className="mb-2 text-xs font-medium text-foreground">Columns</h3>
        <div className="divide-y divide-border rounded-xl border border-border bg-background/40">
          {t.columns.map((c) => (
            <div key={c.id} className="flex items-center gap-3 px-4 py-2">
              <span className="min-w-0 flex-1 truncate font-mono text-xs text-foreground">{c.name}</span>
              <Badge>{typeText(c.type)}</Badge>
              {c.required && <span className="text-2xs text-muted">required</span>}
            </div>
          ))}
        </div>
      </section>
      <section>
        <h3 className="mb-2 text-xs font-medium text-foreground">History</h3>
        {t.snapshots.length === 0 ? (
          <p className="text-xs text-muted">Nothing has been written to this table yet.</p>
        ) : (
          <div className="divide-y divide-border rounded-xl border border-border bg-background/40">
            {t.snapshots.map((s) => (
              <div key={s.id} className="flex items-center gap-3 px-4 py-2 text-xs">
                <span className="flex-1 text-foreground">{s.operation ?? "change"}</span>
                {s.records && <span className="text-muted">{Number(s.records).toLocaleString()} rows</span>}
                <span className="text-muted">{when(s.at_ms)}</span>
                {s.id === t.current_snapshot && <Badge tone="success">current</Badge>}
              </div>
            ))}
          </div>
        )}
      </section>
      <section>
        <h3 className="mb-2 text-xs font-medium text-foreground">Use it from DuckDB</h3>
        <pre className="scroll-area whitespace-pre-wrap break-all rounded-md border border-border bg-background/50 p-3 font-mono text-xs text-foreground">{connect}</pre>
        <div className="mt-2 flex justify-end">
          <Button
            variant="secondary"
            size="sm"
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(connect);
                toast.success("Copied");
              } catch {
                toast.error("Could not copy", "Select the text and copy it.");
              }
            }}
          >
            <Copy /> Copy
          </Button>
        </div>
      </section>
    </div>
  );
}
