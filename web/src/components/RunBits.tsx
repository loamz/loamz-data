import { Square } from "lucide-react";
import { useEffect } from "react";
import { Badge, Button, DetailBlock, DetailRow, toast } from "@/design";
import * as api from "@/api";
import { when } from "@/format";
import { useLoad } from "@/hooks";
import { principalLabel } from "@/principals";

export const stateTone = (s: api.RunState) => (s === "succeeded" ? "success" : s === "failed" || s === "timed_out" ? "danger" : s === "running" ? "accent" : "neutral");
export const stateWords = (s: api.RunState) => s.replace("_", " ");

/** A span of milliseconds the way people read it: 0.4 s, 12 s, 3 min 5 s. Empty while it is not known. */
export function span(ms: number): string {
  if (ms < 0) return "";
  if (ms < 10_000) return `${(Math.round(ms / 100) / 10).toFixed(1)} s`;
  if (ms < 60_000) return `${Math.round(ms / 1000)} s`;
  const m = Math.floor(ms / 60_000);
  return `${m} min ${Math.round((ms % 60_000) / 1000)} s`;
}

export const StateBadge = ({ state }: { state: api.RunState }) => <Badge tone={stateTone(state)}>{stateWords(state)}</Badge>;

export const who = (r: Pick<api.Run, "requested_by" | "on_behalf_of">) => principalLabel(`user:${r.on_behalf_of || r.requested_by}`);

export function RestrictedOutput({ run }: { run: api.Run }) {
  if (run.result_access !== "restricted") return null;
  return <p className="text-sm text-muted">Output is unavailable because access to the input data could not be verified.</p>;
}

/** One run, read from the server and kept current while it goes. */
export function useRun(ws: string, id: string | null) {
  const run = useLoad(() => (id ? api.run(ws, id) : Promise.resolve(null)), `${ws}/${id ?? "-"}`);
  const going = run.data ? api.isRunning(run.data) : false;
  const { reload } = run;
  useEffect(() => {
    if (!going) return;
    const t = setInterval(reload, 1500);
    return () => clearInterval(t);
  }, [going, reload]);
  return run;
}

/** What a run is, in a rail: who, where, when and for how long. */
export function RunFacts({ run, computeName }: { run: api.Run; computeName: (id: string) => string }) {
  return (
    <DetailBlock title="Run">
      <DetailRow label="Status"><StateBadge state={run.state} /></DetailRow>
      <DetailRow label="By">{who(run)}</DetailRow>
      <DetailRow label="Compute">{computeName(run.target_id)}</DetailRow>
      <DetailRow label="Asked">{when(run.created_ms)}</DetailRow>
      {run.queued_ms >= 0 && <DetailRow label="Waited">{span(run.queued_ms)}</DetailRow>}
      {run.run_ms >= 0 && <DetailRow label="Ran">{span(run.run_ms)}</DetailRow>}
      {run.spec?.tables && run.spec.tables.length > 0 && <DetailRow label="Read">{run.spec.tables.map((t) => t.name).join(", ")}</DetailRow>}
      {run.spec?.inputs && run.spec.inputs.length > 0 && <DetailRow label="Read">{run.spec.inputs.map((i) => i.name).join(", ")}</DetailRow>}
    </DetailBlock>
  );
}

export function StopButton({ ws, run, onStopped }: { ws: string; run: api.Run; onStopped: () => void }) {
  if (!api.isRunning(run)) return null;
  return (
    <Button
      variant="secondary"
      size="sm"
      disabled={run.cancelling}
      onClick={async () => {
        try {
          await api.cancelRun(ws, run.id);
          onStopped();
        } catch (e) {
          toast.error("Could not stop it", (e as Error).message);
        }
      }}
    >
      <Square /> {run.cancelling ? "Stopping…" : "Stop"}
    </Button>
  );
}

export function Text({ title, text, danger }: { title: string; text: string; danger?: boolean }) {
  if (!text.trim()) return null;
  return (
    <section className="flex flex-col gap-1.5">
      <h2 className="text-sm font-semibold">{title}</h2>
      <pre className={`scroll-area max-h-80 whitespace-pre-wrap break-words rounded-md border border-border bg-background/50 p-3 font-mono text-xs ${danger ? "text-danger" : "text-foreground"}`}>{text}</pre>
    </section>
  );
}
