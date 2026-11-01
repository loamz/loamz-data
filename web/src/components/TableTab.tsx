import { Copy, Database, Eye } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Principal } from "@/components/Principal";
import { Badge, Button, DataTable, DetailBlock, DetailRow, PageEmpty, Popover, PopoverContent, PopoverTrigger, WithRail, toast, type Column } from "@/design";
import * as api from "@/api";
import { useLoad } from "@/hooks";
import { ResultGrid } from "@/components/ResultGrid";
import { AttachCompute, ComputePicker, useCompute, useWaitingOn } from "@/components/ComputePicker";
import { elapsedText, useClock } from "@/clock";
import { useStatement } from "@/components/useStatement";
import { bytes, when } from "@/format";
import { centralOrigin } from "@/site";

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
  const { target, loaded, running } = compute;
  const [attaching, setAttaching] = useState(false);
  // Read when the tab opens, and again when another compute is chosen: only on compute that is running. A stopped one is started
  // by hand (Select compute), never by a read arriving.
  useEffect(() => {
    if (full && loaded && target && running) void start(`SELECT * FROM ${full} LIMIT 100`, 100, target);
  }, [full, start, target, loaded, running]);
  if (!full) return <p className="text-xs text-muted">This table's full name is still loading.</p>;
  if (loaded && !running) {
    return (
      <>
        <PageEmpty icon={Database} title="Sample data is not available without a running compute" action={<Button variant="secondary" onClick={() => setAttaching(true)}>Select compute</Button>}>
          {compute.all.length === 0 ? "There is no compute yet. Create one on the Compute page." : target ? `${compute.current?.name ?? "The compute"} is stopped. Start it to read rows.` : "Choose a compute, and start it if it is stopped."}
        </PageEmpty>
        <AttachCompute ws={ws} compute={compute} open={attaching} onOpenChange={setAttaching} />
      </>
    );
  }
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

const typeText = (t: string | object) => (typeof t === "string" ? t : (t as { type?: string }).type ?? "nested");

interface ColumnRow {
  id: number;
  name: string;
  type: string;
  values: string;
  nulls: string;
  min: string;
  max: string;
}

/** A table's columns with what its files recorded about them (no compute: a moment's wait), and what the table is in a rail beside it. */
export function TableOverview({ ws, id, catalog, owner, created }: { ws: string; id: string; catalog: string; owner: string; created: number }) {
  const info = useLoad(() => api.tableInfo(ws, id), `${ws}/${id}`);
  const stats = useLoad(() => api.tableStats(ws, id), `${ws}/${id}/stats`);
  const t0 = useRef(Date.now());
  if (info.error && !info.data) {
    return (
      <PageEmpty icon={Database} title="You can't open this table">
        {info.error.message}
      </PageEmpty>
    );
  }
  const t = info.data;
  if (!t) return <p className="text-sm text-muted">Loading…</p>;
  const byId = new Map((stats.data?.columns ?? []).map((c) => [c.id, c]));
  const rows: ColumnRow[] = t.columns.map((c) => {
    const st = byId.get(c.id);
    return {
      id: c.id,
      name: c.name,
      type: typeText(c.type),
      values: st ? st.values.toLocaleString() : "",
      nulls: st ? st.nulls.toLocaleString() : "",
      min: st?.min ?? "",
      max: st?.max ?? "",
    };
  });
  const columns: Column<ColumnRow>[] = [
    { key: "name", header: "Column", width: "fill", cell: (r) => <span className="font-mono">{r.name}</span> },
    { key: "type", header: "Type", cell: (r) => <Badge>{r.type}</Badge> },
    { key: "values", header: "Values", align: "right", cell: (r) => r.values },
    { key: "nulls", header: "Empty", align: "right", cell: (r) => r.nulls },
    { key: "min", header: "Smallest", width: 12, cell: (r) => r.min },
    { key: "max", header: "Largest", width: 12, cell: (r) => r.max },
  ];
  const connect = `INSTALL iceberg; LOAD iceberg;
ATTACH '${catalog}' AS ${catalog} (TYPE iceberg, ENDPOINT '${centralOrigin}/api/v1/workspaces/${ws}/iceberg', TOKEN '<your access token>');`;
  return (
    <WithRail
      rail={
        <>
          <DetailBlock title="About this table">
            <DetailRow label="Rows">{t.stats.records === null ? "—" : t.stats.records.toLocaleString()}</DetailRow>
            <DetailRow label="Files">{t.stats.data_files === null ? "—" : t.stats.data_files.toLocaleString()}</DetailRow>
            <DetailRow label="Size">{t.stats.size_bytes === null ? "—" : bytes(t.stats.size_bytes)}</DetailRow>
            <DetailRow label="Written">{t.stats.at_ms ? when(t.stats.at_ms) : "never"}</DetailRow>
            <DetailRow label="Owner"><Principal id={owner} /></DetailRow>
            <DetailRow label="Created">{when(created)}</DetailRow>
          </DetailBlock>
          <DetailBlock title="Use it elsewhere">
            <Popover>
              <PopoverTrigger asChild>
                <Button variant="secondary" size="sm" className="self-start">
                  <Copy /> DuckDB connection
                </Button>
              </PopoverTrigger>
              <PopoverContent className="w-[34rem] max-w-[90vw]">
                <pre className="scroll-area whitespace-pre-wrap break-all font-mono text-xs text-foreground">{connect}</pre>
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
              </PopoverContent>
            </Popover>
          </DetailBlock>
        </>
      }
    >
      <DataTable columns={columns} rows={rows} rowKey={(r) => String(r.id)} count={`${rows.length} ${rows.length === 1 ? "column" : "columns"}${stats.data ? ` · from the table's metadata, ${elapsedText(Date.now() - t0.current)}` : ""}`} />
    </WithRail>
  );
}

interface Snap {
  id: string;
  at_ms: number;
  operation?: string;
  records?: string;
  files?: string;
}

/** Every change made to the table, newest first: what was done, how many rows it added, and when. */
export function TableHistory({ ws, id }: { ws: string; id: string }) {
  const info = useLoad(() => api.tableInfo(ws, id), `${ws}/${id}`);
  const t = info.data;
  if (!t) return <p className="text-sm text-muted">Loading…</p>;
  const rows: Snap[] = [...t.snapshots].sort((a, b) => b.at_ms - a.at_ms);
  const columns: Column<Snap>[] = [
    { key: "at", header: "When", cell: (r) => when(r.at_ms) },
    { key: "op", header: "Change", width: "fill", cell: (r) => r.operation ?? "change" },
    { key: "records", header: "Rows added", align: "right", cell: (r) => (r.records ? Number(r.records).toLocaleString() : "") },
    { key: "files", header: "Files", align: "right", cell: (r) => r.files ?? "" },
    { key: "now", header: "", cell: (r) => (r.id === t.current_snapshot ? <Badge tone="success">current</Badge> : null) },
  ];
  return <DataTable columns={columns} rows={rows} rowKey={(r) => r.id} empty="Nothing has been written to this table yet." />;
}
