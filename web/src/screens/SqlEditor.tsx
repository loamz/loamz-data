import { Play, Plus, Save, Square, X } from "lucide-react";
import { useCallback, useRef, useState } from "react";
import { Badge, Button, Dialog, DialogContent, DialogFooter, Input, Select, cn, toast } from "@/design";
import * as api from "@/api";
import { elapsedText, useClock } from "@/clock";
import { AttachCompute, ComputePicker, useCompute, useWaitingOn } from "@/components/ComputePicker";
import { CodeEditor, type CodeEditorHandle } from "@/components/CodeEditor";
import { ResultGrid } from "@/components/ResultGrid";
import { EditorPane } from "@/components/EditorPane";
import { go } from "@/route";
import { blank, load, save, titleOf, withSaved, withStatement, type Tab } from "@/sqlTabs";

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

const seconds = elapsedText;

/** SQL on the workspace's compute: tabs of statements, the tables beside them, rows beneath. */
export function SqlEditor({ ws }: { ws: string }) {
  const [state, setState] = useState(() => load(ws));
  const [names, setNames] = useState<string[]>([]);
  const [limit, setLimit] = useState("1000");
  // Which compute to run on: shared with the table pages, and remembered here.
  const picked = useCompute(ws);
  const [outcomes, setOutcomes] = useState<Record<string, Outcome>>({});
  const [naming, setNaming] = useState<string | null>(null);
  const [attaching, setAttaching] = useState(false);
  const handle = useRef<CodeEditorHandle | null>(null);
  const cancelling = useRef<Record<string, boolean>>({});

  const tab = state.tabs.find((t) => t.id === state.active) ?? state.tabs[0]!;
  const out = outcomes[tab.id];
  const clock = useClock(out?.running === true);
  const doing = useWaitingOn(picked, out?.running === true);
  const { online, loaded } = picked;

  const update = (next: typeof state) => {
    setState(next);
    save(ws, next);
  };
  const edit = (patch: Partial<Tab>) => update({ ...state, tabs: state.tabs.map((t) => (t.id === tab.id ? { ...t, ...patch } : t)) });

  const set = useCallback((id: string, o: Partial<Outcome>) => setOutcomes((all) => ({ ...all, [id]: { ...(all[id] ?? { running: false, since: Date.now() }), ...o } })), []);

  const run = async (selection: string | null) => {
    const sql = (selection ?? tab.sql).trim();
    if (!sql) return;
    if (!picked.target) {
      toast.error("Choose a compute first", "Pick the compute this statement should run on, next to Run.");
      return;
    }
    if (!picked.running) {
      setAttaching(true);
      return;
    }
    const id = tab.id;
    if (outcomes[id]?.running) return;
    cancelling.current[id] = false;
    const started = Date.now();
    set(id, { running: true, run: undefined, error: undefined, since: started, elapsed: undefined });
    try {
      // Asked without waiting, so the screen learns at once that the run is queued, then running, instead of after the wait.
      let r = await api.runStatement(ws, sql, Number(limit), 0, picked.target);
      set(id, { run: r });
      // Keep asking until it ends (a cold start can take ten seconds or more).
      while (api.isRunning(r) && !cancelling.current[id]) {
        await new Promise((ok) => setTimeout(ok, 350));
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
  const dirty = !tab.saved || tab.saved.sql !== tab.sql;
  const saveAs = async (name: string) => {
    try {
      const draft = { name, sql: tab.sql, ...(picked.target && { target_id: picked.target }) };
      const q = tab.saved ? await api.changeQuery(ws, tab.saved.id, draft) : await api.saveQuery(ws, draft);
      edit({ title: q.name, saved: { id: q.id, name: q.name, sql: q.sql } });
      toast.success(`Saved ${q.name}`);
      setNaming(null);
    } catch (e) {
      toast.error("Could not save it", (e as Error).message);
    }
  };
  const failed = out?.run && (out.run.state === "failed" || out.run.state === "timed_out");
  const result = out?.run?.state === "succeeded" ? out.run.result : null;
  const noCompute = loaded && picked.all.length === 0;
  // The lists beside the editor refresh when a run starts or ends, and when a query is saved.
  const refreshKey = `${Object.entries(outcomes).map(([id, o]) => `${id}:${o.run?.id ?? ""}:${o.running ? 1 : 0}`).join("|")}#${state.tabs.map((t) => t.saved?.id ?? "").join(",")}`;

  return (
    <div className="flex h-full min-h-0">
      <div className="hidden md:block">
        <EditorPane
          ws={ws}
          refreshKey={refreshKey}
          onPick={insert}
          onNames={setNames}
          onOpenQuery={(q) => update(withSaved(state, q))}
          onOpenStatement={(sql) => update(withStatement(state, sql, titleOf(sql, "Query")))}
        />
      </div>

      <section className="flex min-w-0 flex-1 flex-col">
        <div className="flex shrink-0 items-center gap-1 overflow-x-auto border-b border-border bg-sidebar px-2 py-1">
          {state.tabs.map((t) => (
            <div key={t.id} className={cn("group flex h-control-sm shrink-0 items-center rounded-md text-xs transition-colors", t.id === tab.id ? "bg-card text-foreground shadow-sm" : "text-muted hover:text-foreground")}>
              <button type="button" className="h-full max-w-48 cursor-pointer truncate pl-3 pr-1.5" onClick={() => update({ ...state, active: t.id })}>
                {t.saved ? t.saved.name : titleOf(t.sql, t.title)}
                {t.saved && t.saved.sql !== t.sql && <span className="ml-1.5 inline-block size-1.5 rounded-full bg-muted" aria-label="unsaved changes" />}
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

        <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border px-3 py-1">
          {out?.running ? (
            <Button variant="secondary" size="sm" onClick={() => void stop()}>
              <Square /> Stop
            </Button>
          ) : (
            <Button variant="primary" size="sm" onClick={() => void run(handle.current?.selection() ?? null)} disabled={!tab.sql.trim() || !picked.target} title={picked.target ? (picked.running ? undefined : "Start the compute first") : "Choose a compute first"}>
              <Play /> Run
            </Button>
          )}
          <Button variant="secondary" size="sm" disabled={!tab.sql.trim() || !dirty} onClick={() => (tab.saved ? void saveAs(tab.saved.name) : setNaming(titleOf(tab.sql, tab.title)))}>
            <Save /> Save
          </Button>
          <span className="flex-1" />
          <Select size="sm" className="w-32" aria-label="Rows to return" value={limit} onValueChange={setLimit} options={LIMITS} />
          <ComputePicker compute={picked} />
          {noCompute && (
            <button type="button" onClick={() => go({ screen: "compute", ws })} className="cursor-pointer" title="Create compute">
              <Badge tone="warning">No compute · create one</Badge>
            </button>
          )}
        </div>

        <div className="h-[38%] min-h-32 shrink-0 border-b border-border">
          <CodeEditor key={tab.id} value={tab.sql} onChange={(sql) => edit({ sql })} onRun={(sel) => void run(sel)} language="sql" names={names} hint="SELECT * FROM catalog.schema.table LIMIT 100" handle={handle} />
        </div>

        <div className="min-h-0 flex-1">
          {out?.error ? (
            <Problem title="This could not be run" text={out.error} />
          ) : out?.run?.result_access === "restricted" ? (
            <Problem title="Results unavailable" text="Access to the input data could not be verified." />
          ) : failed ? (
            <Problem title={out.run?.state === "timed_out" ? "It took too long" : "The statement failed"} text={out.run?.error || out.run?.stderr || "No details were reported."} />
          ) : out?.run?.state === "cancelled" ? (
            <Empty text="Stopped." />
          ) : result ? (
            <ResultGrid result={result} elapsed={out?.elapsed} />
          ) : out?.running ? (
            <Empty text={`${doing}… ${elapsedText(clock - out.since)}`} busy />
          ) : (
            <Empty text={noCompute ? "No compute is answering. An account admin attaches a machine in the account console, and starts its agent." : "Run a statement to see its rows here."} />
          )}
        </div>
      </section>

      <AttachCompute ws={ws} compute={picked} open={attaching} onOpenChange={setAttaching} />
      <Dialog open={naming !== null} onOpenChange={(o) => !o && setNaming(null)}>
        <DialogContent title="Save this query" description="Saved queries are yours; a workspace admin can see them too.">
          <form
            className="mt-4 space-y-4"
            onSubmit={(e) => {
              e.preventDefault();
              if (naming?.trim()) void saveAs(naming.trim());
            }}
          >
            <Input autoFocus aria-label="Name" value={naming ?? ""} onChange={(e) => setNaming(e.target.value)} />
            <DialogFooter>
              <Button type="submit" variant="primary" disabled={!naming?.trim()}>
                Save
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
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
