import { Cpu, Play, Plus, Server, Square, TerminalSquare, Trash2, Zap } from "lucide-react";
import { useEffect, useState } from "react";
import { Badge, Button, Dialog, DialogContent, Page, PageEmpty, Segmented, Tabs, TabsContent, TabsList, TabsTrigger, confirmAction, toast } from "@/design";
import * as api from "@/api";
import { CreateCompute } from "@/components/CreateCompute";
import { NewJob } from "@/components/NewJob";
import { computeState, sizeLabel, stateLabel, stateNote } from "@/computeState";
import { go } from "@/route";
import { when } from "@/format";
import { useLoad } from "@/hooks";
import { principalLabel } from "@/principals";

const tone = (s: api.RunState) => (s === "succeeded" ? "success" : s === "failed" || s === "timed_out" ? "danger" : s === "running" ? "accent" : "neutral");
const words = (s: api.RunState) => s.replace("_", " ");
const took = (r: api.Run) => (r.started_ms && r.finished_ms ? `${Math.max(0, Math.round((r.finished_ms - r.started_ms) / 100) / 10)} s` : "");

/** Jobs and statements that have run here, and the compute they run on. */
export function Compute({ ws, me }: { ws: string; me: api.Me | undefined }) {
  const info = me?.workspaces.find((w) => w.id === ws);
  const [making, setMaking] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const [kind, setKind] = useState<"job" | "sql" | "all">("job");
  const [creating, setCreating] = useState(false);
  const [open, setOpen] = useState<string | null>(null);
  const runs = useLoad(() => api.runs(ws, kind === "all" ? undefined : kind, 50), `${ws}/${kind}`);
  const targets = useLoad(() => api.computeTargets(ws), ws);

  // While anything is queued or running the lists keep themselves current, and a warehouse notices it has gone idle.
  const live = (runs.data ?? []).some(api.isRunning);
  useEffect(() => {
    const t = setInterval(() => {
      setNow(Date.now());
      if (live) {
        runs.reload();
        targets.reload();
      }
    }, live ? 2500 : 5000);
    return () => clearInterval(t);
  }, [live, runs.reload, targets.reload]);

  const items = runs.data ?? [];
  return (
    <Page
      title="Compute"
      subtitle="Where statements and jobs run: serverless SQL warehouses that start when needed, and machines of your own. Each run is in a sandbox of its own."
      actions={
        <>
          {info?.admin && (
            <Button variant="primary" onClick={() => setMaking(true)}>
              <Plus /> Create compute
            </Button>
          )}
          <Button variant="secondary" onClick={() => setCreating(true)}>
            <Plus /> New job
          </Button>
        </>
      }
    >
      <Tabs defaultValue="compute" className="space-y-4">
        <TabsList>
          <TabsTrigger value="compute">Compute</TabsTrigger>
          <TabsTrigger value="runs">Jobs and runs</TabsTrigger>
        </TabsList>

        <TabsContent value="runs" className="space-y-3">
          <div className="flex items-center justify-between gap-2">
            <p className="text-xs text-muted">{runs.loading && !runs.data ? "Loading…" : `${items.length} shown, newest first`}</p>
            <Segmented label="Show" value={kind} onChange={setKind} options={[{ value: "job", label: "Jobs" }, { value: "sql", label: "Queries" }, { value: "all", label: "All" }]} />
          </div>
          {items.length === 0 && !runs.loading ? (
            <PageEmpty icon={Play} title={kind === "sql" ? "No queries yet" : "No jobs yet"} action={kind !== "sql" ? <Button variant="primary" onClick={() => setCreating(true)}><Plus /> New job</Button> : undefined}>
              {kind === "sql" ? "Statements you run in the SQL editor are listed here." : "A job runs a Python script over tables and volumes you choose, and saves what it writes."}
            </PageEmpty>
          ) : (
            <div className="divide-y divide-border rounded-xl border border-border bg-background/40">
              {items.map((r) => (
                <button key={r.id} type="button" onClick={() => setOpen(r.id)} className="flex w-full cursor-pointer items-center gap-3 px-4 py-2.5 text-left transition-colors hover:bg-card-hover">
                  <Badge tone={tone(r.state)}>{words(r.state)}</Badge>
                  <span className="min-w-0 flex-1">
                    <span className={`block truncate text-sm text-foreground ${r.kind === "sql" ? "font-mono text-xs" : "font-medium"}`}>{r.title}</span>
                    <span className="block truncate text-2xs text-muted">{r.kind === "sql" ? "Query" : "Job"} · {principalLabel(`user:${r.on_behalf_of || r.requested_by}`)}</span>
                  </span>
                  <span className="hidden w-16 shrink-0 text-right text-xs tabular-nums text-muted sm:block">{took(r)}</span>
                  <span className="w-20 shrink-0 text-right text-2xs text-muted">{when(r.created_ms)}</span>
                </button>
              ))}
            </div>
          )}
        </TabsContent>

        <TabsContent value="compute" className="space-y-3">
          {(targets.data ?? []).length === 0 && !targets.loading ? (
            <PageEmpty icon={Cpu} title="No compute yet" action={info?.admin ? <Button variant="primary" onClick={() => setMaking(true)}><Plus /> Create compute</Button> : undefined}>
              {info?.admin ? "Create a serverless SQL warehouse (it starts when a statement arrives) or attach a machine of your own. Nothing can run until then." : "An admin of this workspace creates compute. Nothing can run until then."}
            </PageEmpty>
          ) : (
            <div className="overflow-hidden rounded-xl border border-border bg-background/40">
              <div className="hidden grid-cols-[minmax(0,2fr)_9rem_8rem_9rem_auto] gap-3 border-b border-border px-4 py-2 text-2xs font-medium uppercase tracking-wide text-muted md:grid">
                <span>Name</span>
                <span>Type</span>
                <span>State</span>
                <span>Size</span>
                <span className="w-24 text-right">Last used</span>
              </div>
              <div className="divide-y divide-border">
                {(targets.data ?? []).map((c) => {
                  const state = computeState(c, now);
                  const Icon = c.kind === "serverless" ? Zap : Server;
                  return (
                    <div key={c.id} className="grid grid-cols-1 items-center gap-1 px-4 py-3 md:grid-cols-[minmax(0,2fr)_9rem_8rem_9rem_auto] md:gap-3">
                      <span className="flex min-w-0 items-center gap-2">
                        <Icon className="shrink-0 text-muted" aria-hidden />
                        <span className="min-w-0">
                          <span className="block truncate text-sm font-medium text-foreground">{c.name}</span>
                          {stateNote[state] && <span className="block truncate text-2xs text-muted">{stateNote[state]}</span>}
                        </span>
                      </span>
                      <span className="text-xs text-muted">{c.kind === "serverless" ? "Serverless SQL warehouse" : "Machine"}</span>
                      <span>
                        <Badge tone={state === "running" ? "success" : "neutral"}>{stateLabel[state]}</Badge>
                      </span>
                      <span className="text-xs text-muted">{sizeLabel(c)}</span>
                      <span className="flex items-center justify-end gap-1">
                        <span className="w-16 text-right text-2xs text-muted">{c.last_used_ms ? when(c.last_used_ms) : "never"}</span>
                        <Button variant="secondary" size="sm" onClick={() => go({ screen: "sql", ws })}>
                          <TerminalSquare /> Query
                        </Button>
                        {info?.admin && (
                          <Button
                            variant="danger"
                            size="icon-sm"
                            aria-label={`Delete ${c.name}`}
                            onClick={async () => {
                              if (!(await confirmAction({ title: `Delete ${c.name}?`, description: "What is waiting or running on it fails.", confirmLabel: "Delete", danger: true }))) return;
                              try {
                                await api.detachCompute(info.org_id, c.id);
                                targets.reload();
                              } catch (e) {
                                toast.error("Could not delete it", (e as Error).message);
                              }
                            }}
                          >
                            <Trash2 />
                          </Button>
                        )}
                      </span>
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </TabsContent>
      </Tabs>

      {info && <CreateCompute org={info.org_id} open={making} onOpenChange={setMaking} onCreated={targets.reload} />}
      <NewJob ws={ws} open={creating} onOpenChange={setCreating} onStarted={(r) => { toast.success("Job started"); setKind("job"); runs.reload(); setOpen(r.id); }} />
      <RunDialog ws={ws} id={open} onClose={() => { setOpen(null); runs.reload(); }} />
    </Page>
  );
}

/** One run: how it went, what it printed, and what it was asked to do. Keeps itself current while it is still going. */
function RunDialog({ ws, id, onClose }: { ws: string; id: string | null; onClose: () => void }) {
  const run = useLoad(() => (id ? api.run(ws, id) : Promise.resolve(null)), `${ws}/${id ?? "-"}`);
  const r = run.data;
  const going = r ? api.isRunning(r) : false;
  useEffect(() => {
    if (!going) return;
    const t = setInterval(run.reload, 1500);
    return () => clearInterval(t);
  }, [going, run.reload]);
  return (
    <Dialog open={id !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent title={r ? r.title : "Run"} description={r ? `${r.kind === "sql" ? "Query" : "Job"} · ${words(r.state)}${took(r) ? ` · ${took(r)}` : ""}` : undefined} className="max-w-2xl">
        {run.error && !r ? <p className="mt-4 text-sm text-danger">{run.error.message}</p> : !r ? <p className="mt-4 text-xs text-muted">Loading…</p> : (
          <div className="mt-4 space-y-4">
            <div className="flex items-center gap-2">
              <Badge tone={tone(r.state)}>{words(r.state)}</Badge>
              {r.cancelling && <span className="text-xs text-muted">Stopping…</span>}
              <span className="flex-1" />
              {going && (
                <Button variant="secondary" onClick={async () => { try { await api.cancelRun(ws, r.id); run.reload(); } catch (e) { toast.error("Could not stop it", (e as Error).message); } }}>
                  <Square /> Stop
                </Button>
              )}
            </div>
            {r.error && <p className="whitespace-pre-wrap text-sm text-danger">{r.error}</p>}
            {r.spec?.inputs && r.spec.inputs.length > 0 && <p className="text-xs text-muted">Read: {r.spec.inputs.map((i) => i.name).join(", ")}</p>}
            {r.kind === "job" && r.state === "succeeded" && <p className="text-xs text-muted">Its files are in the output volume, under <span className="font-mono">runs/{r.id}/</span>.</p>}
            <Section title="Output" text={r.stdout ?? ""} />
            <Section title="Errors" text={r.stderr ?? ""} tone="danger" />
            {r.kind === "job" && <Section title="Script" text={r.spec?.script ?? ""} />}
            {r.kind === "sql" && <Section title="Statement" text={r.spec?.sql ?? ""} />}
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

function Section({ title, text, tone }: { title: string; text: string; tone?: "danger" }) {
  if (!text.trim()) return null;
  return (
    <div className="space-y-1">
      <p className="text-xs font-medium text-foreground">{title}</p>
      <pre className={`scroll-area max-h-48 whitespace-pre-wrap break-words rounded-md border border-border bg-background/50 p-3 font-mono text-xs ${tone === "danger" ? "text-danger" : "text-foreground"}`}>{text}</pre>
    </div>
  );
}
