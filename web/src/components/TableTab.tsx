import { Copy, Database, Eye, Sigma } from "lucide-react";
import { useEffect } from "react";
import { Badge, Button, PageEmpty, toast } from "@/design";
import * as api from "@/api";
import { useLoad } from "@/hooks";
import { ResultGrid } from "@/components/ResultGrid";
import { ComputePicker, useCompute, useWaitingOn } from "@/components/ComputePicker";
import { elapsedText, useClock } from "@/clock";
import { useStatement } from "@/components/useStatement";
import { when } from "@/format";
import { centralOrigin } from "@/site";

const typeText = (t: string | object) => (typeof t === "string" ? t : (t as { type?: string }).type ?? "nested");

/** What a table is: its columns and its history. The data is read with SQL, or by any Iceberg client through this workspace's catalog. */
const quote = (n: string) => `"${n.replace(/"/g, '""')}"`;
const scalar = (t: string | object) => typeof t === "string" && !/^(binary|fixed)/.test(t);

/** A statement that profiles every plain column in one pass: how many rows have a value, how many differ, the smallest and the largest. */
export function profileSql(full: string, columns: api.TableInfo["columns"]): string {
  const cols = columns.filter((c) => scalar(c.type));
  const parts = cols.flatMap((c, i) => [`count(${quote(c.name)}) AS n${i}`, `approx_count_distinct(${quote(c.name)}) AS d${i}`, `CAST(min(${quote(c.name)}) AS VARCHAR) AS lo${i}`, `CAST(max(${quote(c.name)}) AS VARCHAR) AS hi${i}`]);
  return `SELECT count(*) AS total${parts.length ? ", " + parts.join(", ") : ""} FROM ${full}`;
}

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

function Profile({ ws, full, columns }: { ws: string; full: string; columns: api.TableInfo["columns"] }) {
  const s = useStatement(ws);
  const compute = useCompute(ws);
  const now = useClock(s.running);
  const cols = columns.filter((c) => scalar(c.type));
  const row = s.run?.state === "succeeded" ? s.run.result?.rows[0] : undefined;
  return (
    <section>
      <div className="mb-2 flex items-center gap-2">
        <h3 className="flex-1 text-xs font-medium text-foreground">Profile</h3>
        <ComputePicker compute={compute} />
        <Button variant="secondary" size="sm" disabled={s.running || !compute.target} onClick={() => void s.start(profileSql(full, columns), 1, compute.target)}>
          <Sigma /> {s.running ? `Reading… ${s.startedAt ? elapsedText(now - s.startedAt) : ""}` : row ? "Profile again" : "Profile columns"}
        </Button>
      </div>
      {s.error && <p className="text-xs text-danger">{s.error}</p>}
      {row ? (
        <div className="overflow-hidden rounded-xl border border-border">
          <p className="border-b border-border px-4 py-2 text-xs text-muted">{Number(row[0]).toLocaleString()} rows · {elapsedText(s.tookMs ?? 0)}</p>
          <table className="w-full text-left text-xs">
            <thead className="text-muted">
              <tr>
                <th className="px-4 py-1.5 font-medium">Column</th>
                <th className="px-2 py-1.5 text-right font-medium">With a value</th>
                <th className="px-2 py-1.5 text-right font-medium">Different (about)</th>
                <th className="px-2 py-1.5 font-medium">Smallest</th>
                <th className="px-2 py-1.5 font-medium">Largest</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border">
              {cols.map((c, i) => (
                <tr key={c.id}>
                  <td className="px-4 py-1.5 font-mono text-foreground">{c.name}</td>
                  <td className="px-2 py-1.5 text-right tabular-nums">{Number(row[1 + i * 4]).toLocaleString()}</td>
                  <td className="px-2 py-1.5 text-right tabular-nums">{Number(row[2 + i * 4]).toLocaleString()}</td>
                  <td className="max-w-40 truncate px-2 py-1.5">{String(row[3 + i * 4] ?? "")}</td>
                  <td className="max-w-40 truncate px-2 py-1.5">{String(row[4 + i * 4] ?? "")}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        !s.error && <p className="text-xs text-muted">{s.running ? "Running on the compute…" : "Counts, distinct values and the range of each column, in one pass."}</p>
      )}
    </section>
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
      {full && <Profile ws={ws} full={full} columns={t.columns} />}
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
