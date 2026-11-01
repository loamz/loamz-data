import { History } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Body, Button, DataTable, DetailBlock, ObjectHeader, Select, WithRail, type Column } from "@/design";
import * as api from "@/api";
import { RunFacts, StateBadge, StopButton, Text, span, useRun, who } from "@/components/RunBits";
import { when } from "@/format";
import { useLoad } from "@/hooks";
import { openInEditor } from "@/sqlTabs";

const DAY = 86_400_000;
const ranges = [
  { value: "1", label: "Last day" },
  { value: "7", label: "Last 7 days" },
  { value: "30", label: "Last 30 days" },
  { value: "0", label: "Any time" },
];
const states: { value: string; label: string }[] = [
  { value: "any", label: "Any status" },
  ...(["succeeded", "failed", "running", "queued", "cancelled", "timed_out"] as const).map((s) => ({ value: s, label: s.replace("_", " ") })),
];
const PAGE = 50;

/** Every statement and job that ran in the workspace: who, where, how long it waited and how long it ran. */
export function QueryHistory({ ws, me }: { ws: string; me: api.Me | undefined }) {
  const admin = Boolean(me?.workspaces.find((w) => w.id === ws)?.admin);
  const [kind, setKind] = useState("sql");
  const [range, setRange] = useState("7");
  const [state, setState] = useState("any");
  const [target, setTarget] = useState("any");
  const [mine, setMine] = useState(false);
  const [extra, setExtra] = useState<api.Run[]>([]);
  const [more, setMore] = useState(true);
  const [open, setOpen] = useState<string | null>(null);
  const computes = useLoad(() => api.computeTargets(ws), ws);
  const filter = useMemo<api.RunFilter>(
    () => ({
      limit: PAGE,
      ...(kind !== "all" && { kind: kind as "sql" | "job" }),
      ...(state !== "any" && { state: state as api.RunState }),
      ...(target !== "any" && { target }),
      ...(range !== "0" && { since_ms: Date.now() - Number(range) * DAY }),
      ...(mine && me && { by: me.subject }),
    }),
    // `Date.now()` is read when the filter changes, not on every render.
    [kind, state, target, range, mine, me],
  );
  const first = useLoad(() => api.runs(ws, filter), `${ws}/${JSON.stringify(filter)}`);
  useEffect(() => {
    setExtra([]);
    setMore(true);
  }, [filter]);
  const rows = [...(first.data ?? []), ...extra];
  const name = (id: string) => computes.data?.find((c) => c.id === id)?.name ?? "removed";

  const columns: Column<api.Run>[] = [
    { key: "state", header: "Status", cell: (r) => <StateBadge state={r.state} /> },
    { key: "title", header: "Statement", width: "fill", cell: (r) => <span className={r.kind === "sql" ? "font-mono text-xs" : ""}>{r.title}</span> },
    { key: "by", header: "By", cell: (r) => who(r) },
    { key: "on", header: "Compute", cell: (r) => name(r.target_id) },
    { key: "at", header: "Started", cell: (r) => when(r.created_ms) },
    { key: "took", header: "Duration", align: "right", cell: (r) => (r.run_ms >= 0 ? span(r.queued_ms + r.run_ms) : "") },
  ];
  return (
    <Body>
      <ObjectHeader icon={History} title="Query history" />
      <div className="flex flex-wrap items-center gap-2">
        <Select size="sm" className="w-32" aria-label="Kind" value={kind} onValueChange={setKind} options={[{ value: "sql", label: "Queries" }, { value: "job", label: "Jobs" }, { value: "all", label: "Both" }]} />
        <Select size="sm" className="w-36" aria-label="Time" value={range} onValueChange={setRange} options={ranges} />
        <Select size="sm" className="w-36" aria-label="Status" value={state} onValueChange={setState} options={states} />
        <Select size="sm" className="w-44" aria-label="Compute" value={target} onValueChange={setTarget} options={[{ value: "any", label: "Any compute" }, ...(computes.data ?? []).map((c) => ({ value: c.id, label: c.name }))]} />
        {admin && (
          <Select size="sm" className="w-36" aria-label="Who" value={mine ? "me" : "all"} onValueChange={(v) => setMine(v === "me")} options={[{ value: "all", label: "Everyone" }, { value: "me", label: "Only me" }]} />
        )}
      </div>
      <WithRail rail={open ? <Detail ws={ws} id={open} name={name} /> : <></>}>
        <DataTable
          columns={columns}
          rows={rows}
          rowKey={(r) => r.id}
          selected={open}
          onOpen={(r) => setOpen(r.id)}
          count={first.loading && !first.data ? "Loading…" : `${rows.length}${more && rows.length >= PAGE ? "+" : ""} ${rows.length === 1 ? "run" : "runs"}`}
          empty="Nothing ran in this range."
        />
        {more && rows.length >= PAGE && rows.length % PAGE === 0 && (
          <div className="mt-2">
            <Button
              variant="secondary"
              size="sm"
              onClick={async () => {
                const page = await api.runs(ws, { ...filter, before_ms: rows[rows.length - 1]!.created_ms });
                setExtra((e) => [...e, ...page]);
                if (page.length < PAGE) setMore(false);
              }}
            >
              Load more
            </Button>
          </div>
        )}
      </WithRail>
    </Body>
  );
}

function Detail({ ws, id, name }: { ws: string; id: string; name: (id: string) => string }) {
  const run = useRun(ws, id);
  const r = run.data;
  if (!r) return <p className="text-sm text-muted">{run.error ? run.error.message : "Loading…"}</p>;
  return (
    <>
      <div className="flex items-center gap-2">
        <h2 className="min-w-0 flex-1 truncate text-sm font-semibold">{r.kind === "sql" ? "Query" : "Job"}</h2>
        <StopButton ws={ws} run={r} onStopped={run.reload} />
        {r.kind === "sql" && r.spec?.sql && (
          <Button variant="secondary" size="sm" onClick={() => openInEditor(ws, r.spec?.sql ?? "", "Query")}>
            Open in editor
          </Button>
        )}
      </div>
      <RunFacts run={r} computeName={name} />
      {r.error && <DetailBlock title="Error"><p className="whitespace-pre-wrap text-danger">{r.error}</p></DetailBlock>}
      <Text title="Statement" text={r.spec?.sql ?? ""} />
      <Text title="Output" text={r.stdout ?? ""} />
      <Text title="Errors" text={r.stderr ?? ""} danger />
    </>
  );
}
