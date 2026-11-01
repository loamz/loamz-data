import { Copy, Database, Eye, Sigma } from "lucide-react";
import { useEffect, useRef } from "react";
import { Badge, Button, PageEmpty, toast } from "@/design";
import * as api from "@/api";
import { useLoad } from "@/hooks";
import { ResultGrid } from "@/components/ResultGrid";
import { ComputePicker, useCompute, useWaitingOn } from "@/components/ComputePicker";
import { elapsedText, useClock } from "@/clock";
import { useStatement } from "@/components/useStatement";
import { bytes, when } from "@/format";
import { centralOrigin } from "@/site";

const typeText = (t: string | object) => (typeof t === "string" ? t : (t as { type?: string }).type ?? "nested");

/** What a table is: its columns and its history. The data is read with SQL, or by any Iceberg client through this workspace's catalog. */
const quote = (n: string) => `"${n.replace(/"/g, '""')}"`;
const scalar = (t: string | object) => typeof t === "string" && !/^(binary|fixed)/.test(t);

/** The first rows of a table, read as soon as the tab opens (on the workspace's compute; a stopped warehouse starts, which takes a few seconds). */
export function SampleData({ ws, full }: { ws: string; full: string | null }) {
  const s = useStatement(ws);
  const result = s.run?.state === "succeeded" ? s.run.result : undefined;
  const compute = useCompute(ws);
  const now = useClock(s.running);
  const waited = s.startedAt ? elapsedText(now - s.startedAt) : "";
  const doing = useWaitingOn(compute, s.running);
  const { start } = s;
  const { target, loaded } = compute;
  // Read when the tab opens, and again when another compute is chosen.
  useEffect(() => {
    if (full && loaded && target) void start(`SELECT * FROM ${full} LIMIT 100`, 100, target);
  }, [full, start, target, loaded]);
  if (!full) return <p className="text-xs text-muted">This table's full name is still loading.</p>;
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <p className="flex-1 text-xs text-muted">
          {!target
            ? loaded && compute.all.length === 0
              ? "There is no compute yet. Create one on the Compute page."
              : "Choose a compute to read the rows."
            : s.running
              ? `${doing}… ${waited}`
              : result
                ? `The first ${result.rows.length.toLocaleString()} ${result.rows.length === 1 ? "row" : "rows"} · ${elapsedText(s.tookMs ?? 0)}`
                : ""}
        </p>
        <ComputePicker compute={compute} />
        <Button variant="secondary" size="sm" disabled={s.running || !target} onClick={() => void s.start(`SELECT * FROM ${full} LIMIT 100`, 100, target)}>
          <Eye /> Refresh
        </Button>
      </div>
      {s.error && <p className="text-sm text-danger">{s.error}</p>}
      {result && (
        <div className="h-[28rem] overflow-hidden rounded-xl border border-border">
          <ResultGrid result={result} />
        </div>
      )}
    </div>
  );
}

/** Counts and ranges of each column, read from what the table's files recorded about themselves: no compute, a moment's wait. */
function Profile({ ws, id }: { ws: string; id: string }) {
  const stats = useLoad(() => api.tableStats(ws, id), `${ws}/${id}/stats`);
  const t0 = useRef(Date.now());
  const took = stats.data ? elapsedText(Date.now() - t0.current) : "";
  return (
    <section>
      <div className="mb-2 flex items-center gap-2">
        <h3 className="flex-1 text-xs font-medium text-foreground">Columns</h3>
        {stats.data && <span className="text-2xs text-muted">from the table's metadata · {took}</span>}
      </div>
      {stats.error && !stats.data ? (
        <p className="text-xs text-danger">{stats.error.message}</p>
      ) : !stats.data ? (
        <p className="text-xs text-muted">Reading the table's metadata…</p>
      ) : (
        <div className="overflow-hidden rounded-xl border border-border">
          <table className="w-full text-left text-xs">
            <thead className="text-muted">
              <tr>
                <th className="px-4 py-1.5 font-medium">Column</th>
                <th className="px-2 py-1.5 text-right font-medium">Values</th>
                <th className="px-2 py-1.5 text-right font-medium">Empty</th>
                <th className="px-2 py-1.5 font-medium">Smallest</th>
                <th className="px-2 py-1.5 font-medium">Largest</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border">
              {stats.data.columns.map((c) => (
                <tr key={c.id}>
                  <td className="px-4 py-1.5 font-mono text-foreground">{c.name}</td>
                  <td className="px-2 py-1.5 text-right tabular-nums">{c.values.toLocaleString()}</td>
                  <td className="px-2 py-1.5 text-right tabular-nums">{c.nulls.toLocaleString()}</td>
                  <td className="max-w-40 truncate px-2 py-1.5">{c.min ?? ""}</td>
                  <td className="max-w-40 truncate px-2 py-1.5">{c.max ?? ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <p className="text-2xs uppercase tracking-wide text-muted">{label}</p>
      <p className="text-sm font-medium tabular-nums text-foreground">{value}</p>
    </div>
  );
}

export function TableTab({ ws, id, catalog, full }: { ws: string; id: string; catalog: string; full: string | null }) {
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
      <section className="flex flex-wrap gap-x-8 gap-y-2" aria-label="About this table">
        <Fact label="Rows" value={t.stats.records === null ? "—" : t.stats.records.toLocaleString()} />
        <Fact label="Files" value={t.stats.data_files === null ? "—" : t.stats.data_files.toLocaleString()} />
        <Fact label="Size" value={t.stats.size_bytes === null ? "—" : bytes(t.stats.size_bytes)} />
        <Fact label="Last written" value={t.stats.at_ms ? when(t.stats.at_ms) : "never"} />
      </section>
      <section>
        <h3 className="mb-2 text-xs font-medium text-foreground">Schema</h3>
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
      <Profile ws={ws} id={id} />
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
