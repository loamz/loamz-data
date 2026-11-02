import { Button, Tabs, TabsList, TabsTrigger, cn, toast } from "@/design";
import { useState } from "react";
import * as api from "@/api";
import { when } from "@/format";
import { useLoad } from "@/hooks";
import { go } from "@/route";
import { stateWords, span } from "./RunBits";
import { SchemaBrowser } from "./SchemaBrowser";

type Pane = "tables" | "queries" | "history";

const dot = (s: api.RunState) => (s === "succeeded" ? "bg-success" : s === "failed" || s === "timed_out" ? "bg-danger" : s === "running" || s === "queued" ? "animate-pulse bg-accent" : "bg-muted/50");

/**
 * The left side of the SQL editor, as in Databricks: the tables to click into a statement, the queries you saved, and the statements you ran,
 * one at a time. `refreshKey` changes when a run starts, ends or a query is saved, so the lists are never behind what you just did.
 */
export function EditorPane({
  ws,
  refreshKey,
  onPick,
  onNames,
  onOpenQuery,
  onOpenStatement,
}: {
  ws: string;
  refreshKey: string;
  onPick: (fullName: string) => void;
  onNames: (names: string[]) => void;
  onOpenQuery: (q: api.SavedQuery) => void;
  onOpenStatement: (sql: string) => void;
}) {
  const [pane, setPane] = useState<Pane>("tables");
  return (
    <div className="flex h-full min-h-0 w-60 shrink-0 flex-col border-r border-border">
      <Tabs value={pane} onValueChange={(v) => setPane(v as Pane)} className="shrink-0 px-2 pt-1">
        <TabsList variant="line">
          <TabsTrigger value="tables">Tables</TabsTrigger>
          <TabsTrigger value="queries">Queries</TabsTrigger>
          <TabsTrigger value="history">History</TabsTrigger>
        </TabsList>
      </Tabs>
      {/* The tables stay mounted, so the folders you opened are still open when you come back. */}
      <div className={cn("flex min-h-0 flex-1 flex-col pt-1", pane !== "tables" && "hidden")}>
        <SchemaBrowser ws={ws} onPick={onPick} onNames={onNames} />
      </div>
      {pane === "queries" && <Saved ws={ws} refreshKey={refreshKey} onOpen={onOpenQuery} />}
      {pane === "history" && <Recent ws={ws} refreshKey={refreshKey} onOpen={onOpenStatement} />}
    </div>
  );
}

function Foot({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <div className="shrink-0 border-t border-border px-2 py-1">
      <Button variant="ghost" size="sm" className="w-full justify-start text-accent" onClick={onClick}>
        {label}
      </Button>
    </div>
  );
}

function Saved({ ws, refreshKey, onOpen }: { ws: string; refreshKey: string; onOpen: (q: api.SavedQuery) => void }) {
  const list = useLoad(() => api.savedQueries(ws), `${ws}/${refreshKey}`);
  const items = list.data ?? [];
  return (
    <>
      <ul className="scroll-area min-h-0 flex-1 px-1 pb-2 pt-1" aria-label="Saved queries">
        {items.map((q) => (
          <li key={q.id}>
            <button type="button" onClick={() => onOpen(q)} title={q.sql} className="flex w-full cursor-pointer flex-col rounded-md px-2 py-1 text-left hover:bg-card-hover">
              <span className="truncate text-sm">{q.name}</span>
              <span className="text-2xs text-muted">{when(q.updated_ms)}</span>
            </button>
          </li>
        ))}
        {items.length === 0 && <li className="px-2 py-4 text-xs text-muted">{list.error ? list.error.message : list.loading ? "Loading…" : "Nothing saved yet. Save a statement and it is here."}</li>}
      </ul>
      <Foot label="All queries" onClick={() => go({ screen: "queries", ws })} />
    </>
  );
}

function Recent({ ws, refreshKey, onOpen }: { ws: string; refreshKey: string; onOpen: (sql: string) => void }) {
  const me = useLoad(api.me, "me");
  const subject = me.data?.subject;
  // Your own statements: an admin's list would otherwise hold everyone's.
  const list = useLoad(() => (subject ? api.runs(ws, { kind: "sql", by: subject, limit: 50 }) : Promise.resolve([])), `${ws}/${subject ?? ""}/${refreshKey}`);
  const items = list.data ?? [];
  const open = async (r: api.Run) => {
    try {
      // The list carries only a statement's first words; the whole of it is on the one run.
      const full = await api.run(ws, r.id);
      onOpen(full.spec?.sql ?? r.title);
    } catch (e) {
      toast.error("Could not open it", (e as Error).message);
    }
  };
  return (
    <>
      <ul className="scroll-area min-h-0 flex-1 px-1 pb-2 pt-1" aria-label="Your statements">
        {items.map((r) => (
          <li key={r.id}>
            <button type="button" onClick={() => void open(r)} title={r.title} className="flex w-full cursor-pointer flex-col gap-0.5 rounded-md px-2 py-1 text-left hover:bg-card-hover">
              <span className="flex min-w-0 items-start gap-2">
                <span className={cn("mt-1 size-2 shrink-0 rounded-full", dot(r.state))} aria-hidden />
                <span className="line-clamp-2 min-w-0 break-words font-mono text-xs">{r.title}</span>
              </span>
              <span className="pl-4 text-2xs text-muted">
                {stateWords(r.state)} · {when(r.created_ms)}
                {r.run_ms >= 0 && ` · ${span(r.run_ms)}`}
              </span>
            </button>
          </li>
        ))}
        {items.length === 0 && <li className="px-2 py-4 text-xs text-muted">{list.error ? list.error.message : list.loading || me.loading ? "Loading…" : "Nothing run yet."}</li>}
      </ul>
      <Foot label="Open query history" onClick={() => go({ screen: "history", ws })} />
    </>
  );
}
