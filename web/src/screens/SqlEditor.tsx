import { History, Play, Plus, Square, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Badge, Button, Select, cn, toast } from "@/design";
import * as api from "@/api";
import { CodeEditor, type CodeEditorHandle } from "@/components/CodeEditor";
import { ResultGrid } from "@/components/ResultGrid";
import { SchemaBrowser } from "@/components/SchemaBrowser";
import { when } from "@/format";
import { useLoad } from "@/hooks";
import { go } from "@/route";
import { blank, load, save, titleOf, type Tab } from "@/sqlTabs";

/** What one tab last did. Kept while the page is open; the statement itself is kept by the server as a run. */
interface Outcome {
  run?: api.Run;
  error?: string;
  running: boolean;
  since: number;
  elapsed?: string;
}

const LIMITS = [
  { value: "100", label: "100 rows" },
  { value: "1000", label: "1,000 rows" },
  { value: "10000", label: "10,000 rows" },
];

const seconds = (ms: number) => (ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`);

/** SQL on the workspace's compute: tabs of statements, the tables beside them, rows beneath. */
export function SqlEditor({ ws }: { ws: string }) {
  const [state, setState] = useState(() => load(ws));
  const [names, setNames] = useState<string[]>([]);
  const [limit, setLimit] = useState("1000");
  const [outcomes, setOutcomes] = useState<Record<string, Outcome>>({});
  const [bottom, setBottom] = useState<"results" | "history">("results");
  const handle = useRef<CodeEditorHandle | null>(null);
  const cancelling = useRef<Record<string, boolean>>({});

  const tab = state.tabs.find((t) => t.id === state.active) ?? state.tabs[0]!;
  const out = outcomes[tab.id];
  const compute = useLoad(() => api.computeTargets(ws), ws);
  const online = (compute.data ?? []).filter((c) => c.online);

  // The compute list is checked now and then, so the chip is honest without a reload.
  useEffect(() => {
    const t = setInterval(compute.reload, 15_000);
    return () => clearInterval(t);
  }, [compute.reload]);

  const update = (next: typeof state) => {
    setState(next);
    save(ws, next);
  };
  const edit = (patch: Partial<Tab>) => update({ ...state, tabs: state.tabs.map((t) => (t.id === tab.id ? { ...t, ...patch } : t)) });

  const set = useCallback((id: string, o: Partial<Outcome>) => setOutcomes((all) => ({ ...all, [id]: { ...(all[id] ?? { running: false, since: Date.now() }), ...o } })), []);

  const run = async (selection: string | null) => {
    const sql = (selection ?? tab.sql).trim();
    if (!sql) return;
    const id = tab.id;
    if (outcomes[id]?.running) return;
    cancelling.current[id] = false;
    const started = Date.now();
    set(id, { running: true, run: undefined, error: undefined, since: started, elapsed: undefined });
    setBottom("results");
    try {
      let r = await api.runStatement(ws, sql, Number(limit), 25);
      set(id, { run: r });
      // Still going (a cold sandbox, a slow query): keep asking until it ends.
      while (api.isRunning(r) && !cancelling.current[id]) {
        await new Promise((ok) => setTimeout(ok, 1000));
        r = await api.run(ws, r.id);
        set(id, { run: r });
      }
      set(id, { run: r, running: false, elapsed: seconds(Date.now() - started) });
    } catch (e) {
      set(id, { running: false, error: (e as Error).message, elapsed: seconds(Date.now() - started) });
    }
  };

  const stop = async () => {
    const r = out?.run;
    cancelling.current[tab.id] = true;
    if (!r) return set(tab.id, { running: false, error: "Stopped." });
    try {
      set(tab.id, { run: await api.cancelRun(ws, r.id), running: false, elapsed: seconds(Date.now() - out.since) });
    } catch (e) {
      toast.error("Could not stop it", (e as Error).message);
    }
  };

  const add = () => {
    const t = blank(`Query ${state.tabs.length + 1}`);
    update({ tabs: [...state.tabs, t], active: t.id });
  };
  const close = (id: string) => {
    if (state.tabs.length === 1) {
      const fresh = blank("Query 1");
      return update({ tabs: [fresh], active: fresh.id });
    }
    const rest = state.tabs.filter((t) => t.id !== id);
    update({ tabs: rest, active: state.active === id ? rest[0]!.id : state.active });
  };
  const insert = (name: string) => edit({ sql: tab.sql ? `${tab.sql.replace(/\s*$/, "")}\n${name}` : `SELECT * FROM ${name} LIMIT 100`, title: titleOf(tab.sql || name, tab.title) });
  const failed = out?.run && (out.run.state === "failed" || out.run.state === "timed_out");
  const result = out?.run?.state === "succeeded" ? out.run.result : null;
  const noCompute = compute.data !== undefined && online.length === 0;

  return (
    <div className="flex h-full min-h-0">
      <aside className="hidden w-64 shrink-0 flex-col border-r border-border bg-sidebar md:flex">
        <div className="px-4 pb-2 pt-4">
          <h1 className="text-sm font-semibold text-foreground">Tables</h1>
          <p className="text-2xs text-muted">Click one to put its name in the editor.</p>
        </div>
        <SchemaBrowser ws={ws} onPick={insert} onNames={setNames} />
      </aside>

      <section className="flex min-w-0 flex-1 flex-col">
        <div className="flex shrink-0 items-center gap-1 overflow-x-auto border-b border-border bg-sidebar px-2 py-1.5">
          {state.tabs.map((t) => (
            <div key={t.id} className={cn("group flex h-control-sm shrink-0 items-center rounded-md text-xs transition-colors", t.id === tab.id ? "bg-card text-foreground shadow-sm" : "text-muted hover:text-foreground")}>
              <button type="button" className="h-full max-w-48 cursor-pointer truncate pl-3 pr-1.5" onClick={() => update({ ...state, active: t.id })}>
                {titleOf(t.sql, t.title)}
                {outcomes[t.id]?.running && <span className="ml-1.5 inline-block size-1.5 animate-pulse rounded-full bg-accent" aria-label="running" />}
              </button>
              <button type="button" aria-label={`Close ${t.title}`} className="mr-1 flex size-5 cursor-pointer items-center justify-center rounded text-muted-foreground opacity-0 hover:bg-card-hover hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100" onClick={() => close(t.id)}>
                <X className="size-3" />
              </button>
            </div>
          ))}
          <Button variant="ghost" size="icon-sm" aria-label="New query" onClick={add}>
            <Plus />
          </Button>
        </div>

        <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border px-3 py-2">
          {out?.running ? (
            <Button variant="secondary" onClick={() => void stop()}>
              <Square /> Stop
            </Button>
          ) : (
            <Button variant="primary" onClick={() => void run(handle.current?.selection() ?? null)} disabled={!tab.sql.trim()}>
              <Play /> Run
            </Button>
          )}
          <span className="hidden text-2xs text-muted-foreground sm:inline">Ctrl+Enter · runs the selection if there is one</span>
          <span className="flex-1" />
          <Select size="sm" className="w-32" aria-label="Rows to return" value={limit} onValueChange={setLimit} options={LIMITS} />
          <button type="button" onClick={() => go({ screen: "compute", ws })} className="cursor-pointer" title="See the compute">
            {noCompute ? <Badge tone="warning">No compute answering</Badge> : online[0] ? <Badge tone="success">{online[0].name} · online</Badge> : <Badge>Checking compute…</Badge>}
          </button>
        </div>

        <div className="h-[38%] min-h-32 shrink-0 border-b border-border">
          <CodeEditor key={tab.id} value={tab.sql} onChange={(sql) => edit({ sql })} onRun={(sel) => void run(sel)} language="sql" names={names} hint="SELECT * FROM catalog.schema.table LIMIT 100" handle={handle} />
        </div>

        <div className="flex shrink-0 items-center gap-1 border-b border-border px-2 py-1">
          {(["results", "history"] as const).map((b) => (
            <button key={b} type="button" onClick={() => setBottom(b)} className={cn("h-control-sm cursor-pointer rounded-md px-3 text-xs font-medium transition-colors", bottom === b ? "bg-accent/15 text-accent" : "text-muted hover:text-foreground")}>
              {b === "results" ? "Results" : "History"}
            </button>
          ))}
        </div>

        <div className="min-h-0 flex-1">
          {bottom === "history" ? (
            <HistoryList ws={ws} onOpen={(sql) => {
                const t = blank(titleOf(sql, "Query"), sql);
                update({ tabs: [...state.tabs, t], active: t.id });
                setBottom("results");
              }} />
          ) : out?.error ? (
            <Problem title="This could not be run" text={out.error} />
          ) : failed ? (
            <Problem title={out.run?.state === "timed_out" ? "It took too long" : "The statement failed"} text={out.run?.error || out.run?.stderr || "No details were reported."} />
          ) : out?.run?.state === "cancelled" ? (
            <Empty text="Stopped." />
          ) : result ? (
            <ResultGrid result={result} elapsed={out?.elapsed} />
          ) : out?.running ? (
            <Empty text={out.run?.state === "running" ? "Running on the compute…" : "Waiting for the compute…"} busy />
          ) : (
            <Empty text={noCompute ? "No compute is answering. An account admin attaches a machine in the account console, and starts its agent." : "Run a statement to see its rows here."} />
          )}
        </div>
      </section>
    </div>
  );
}

function Empty({ text, busy }: { text: string; busy?: boolean }) {
  return (
    <div className="flex h-full items-center justify-center gap-2 px-6 text-center text-xs text-muted">
      {busy && <span className="size-2 animate-pulse rounded-full bg-accent" aria-hidden />}
      <span>{text}</span>
    </div>
  );
}

function Problem({ title, text }: { title: string; text: string }) {
  return (
    <div className="scroll-area h-full px-4 py-3">
      <p className="text-sm font-medium text-danger">{title}</p>
      <pre className="mt-2 whitespace-pre-wrap break-words font-mono text-xs text-foreground">{text}</pre>
    </div>
  );
}

/** The statements you have run here, newest first. Open one to put it in a new tab. */
function HistoryList({ ws, onOpen }: { ws: string; onOpen: (sql: string) => void }) {
  const list = useLoad(() => api.runs(ws, "sql", 40), ws);
  const items = useMemo(() => list.data ?? [], [list.data]);
  if (list.loading && !list.data) return <Empty text="Loading…" />;
  if (items.length === 0) return <Empty text="Nothing run here yet." />;
  return (
    <div className="scroll-area h-full divide-y divide-border">
      {items.map((r) => (
        <button key={r.id} type="button" className="flex w-full cursor-pointer items-center gap-3 px-4 py-2 text-left transition-colors hover:bg-card-hover" onClick={async () => onOpen((await api.run(ws, r.id).catch(() => r)).spec?.sql ?? r.title)}>
          <History className="shrink-0 text-muted" aria-hidden />
          <span className="min-w-0 flex-1 truncate font-mono text-xs text-foreground">{r.title}</span>
          <Badge tone={r.state === "succeeded" ? "success" : r.state === "failed" || r.state === "timed_out" ? "danger" : "neutral"}>{r.state.replace("_", " ")}</Badge>
          <span className="w-20 shrink-0 text-right text-2xs text-muted">{when(r.created_ms)}</span>
        </button>
      ))}
    </div>
  );
}
