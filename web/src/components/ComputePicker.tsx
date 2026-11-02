import { useCallback, useEffect, useState } from "react";
import { Button, Dialog, DialogContent, DialogFooter, Select, toast } from "@/design";
import * as api from "@/api";
import { computeState, stateLabel } from "@/computeState";
import { StateDot } from "@/components/StateDot";
import { useLoad } from "@/hooks";

const key = (ws: string) => `loamz.compute.${ws}`;

function remembered(ws: string): string {
  try {
    return localStorage.getItem(key(ws)) ?? "";
  } catch {
    return "";
  }
}

/** The compute a statement runs on in this workspace: the one the person chose (remembered here). There is no automatic choice:
 *  nothing runs until a compute is selected, and the one in use is always the one shown. With a single compute available it is
 *  selected for them, visibly. Shared by every screen that runs SQL. */
export function useCompute(ws: string) {
  const targets = useLoad(() => api.computeTargets(ws), ws);
  const [chosen, setChosen] = useState(() => remembered(ws));
  useEffect(() => setChosen(remembered(ws)), [ws]);
  useEffect(() => {
    const t = setInterval(targets.reload, 15_000);
    return () => clearInterval(t);
  }, [targets.reload]);
  // Compute this browser asked to start and that has not answered yet: shown as starting, and not yet usable.
  const [starting, setStarting] = useState<ReadonlySet<string>>(new Set());
  const all = (targets.data ?? []).map((c) => (starting.has(c.id) ? { ...c, starting: true } : c));
  const online = all.filter((c) => c.online);
  const { reload } = targets;
  const start = useCallback(
    async (org: string, id: string, name: string) => {
      setStarting((s) => new Set(s).add(id));
      try {
        await api.startCompute(org, id);
      } catch (e) {
        toast.error(`Could not start ${name}`, (e as Error).message);
      } finally {
        setStarting((s) => {
          const n = new Set(s);
          n.delete(id);
          return n;
        });
        reload();
      }
    },
    [reload],
  );
  // A remembered choice that no longer exists is dropped, not silently replaced by another.
  const valid = all.some((c) => c.id === chosen);
  const only = all.length === 1 ? all[0]!.id : "";
  const target = valid ? chosen : only;
  const choose = useCallback(
    (id: string) => {
      setChosen(id);
      try {
        localStorage.setItem(key(ws), id);
      } catch {
        /* the choice then lasts until the page closes */
      }
    },
    [ws],
  );
  const current = all.find((c) => c.id === target);
  // What a person may run on is only what is running: a stopped warehouse is started by hand, never by a statement arriving.
  const running = current ? computeState(current, Date.now()) === "running" : false;
  return { online, all, current, running, start, target: target || undefined, choose, reload: targets.reload, loaded: targets.data !== undefined };
}

/** What the compute in use is doing while a statement waits on it. */
export function useWaitingOn(compute: ReturnType<typeof useCompute>, running: boolean): string {
  const { current, reload } = compute;
  useEffect(() => {
    if (!running) return;
    const t = setInterval(reload, 2000);
    return () => clearInterval(t);
  }, [running, reload]);
  if (!current) return "the compute";
  return current.starting ? `Starting ${current.name}` : `Running on ${current.name}`;
}

export function ComputePicker({ compute, className = "w-52" }: { compute: ReturnType<typeof useCompute>; className?: string }) {
  const { all, target, choose } = compute;
  return (
    <Select
      size="sm"
      className={className}
      aria-label="Compute"
      placeholder="Select compute"
      value={target ?? ""}
      onValueChange={choose}
      options={all.map((c) => ({ value: c.id, label: `${c.name} · ${c.kind === "serverless" ? "serverless" : "machine"}`, leading: <StateDot state={computeState(c, Date.now())} /> }))}
    />
  );
}

/** Databricks' "Attach to an existing compute resource": pick one, and start it if it is stopped, in one step. */
export function AttachCompute({ ws, compute, open, onOpenChange }: { ws: string; compute: ReturnType<typeof useCompute>; open: boolean; onOpenChange: (o: boolean) => void }) {
  const me = useLoad(api.me, "me");
  const info = me.data?.workspaces.find((w) => w.id === ws);
  const [pick, setPick] = useState("");
  useEffect(() => {
    if (open) setPick(compute.target ?? compute.all[0]?.id ?? "");
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps
  const chosen = compute.all.find((c) => c.id === pick);
  const stopped = chosen ? computeState(chosen, Date.now()) === "stopped" : false;
  const canStart = Boolean(info?.admin);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent title="Attach to an existing compute resource">
        <div className="mt-4 space-y-3">
          <Select
            aria-label="Compute"
            placeholder={compute.all.length === 0 ? "No compute yet" : "Select compute"}
            value={pick}
            onValueChange={setPick}
            options={compute.all.map((c) => ({ value: c.id, label: c.name, leading: <StateDot state={computeState(c, Date.now())} /> }))}
          />
          {chosen && stopped && !canStart && <p className="text-xs text-muted">{chosen.name} is stopped. A workspace admin has to start it.</p>}
          <DialogFooter>
            <Button
              variant="primary"
              disabled={!chosen || (stopped && !canStart)}
              onClick={() => {
                if (!chosen) return;
                // Closes at once; the compute shows as starting (a spinner) until it answers.
                compute.choose(chosen.id);
                onOpenChange(false);
                if (stopped && info) void compute.start(info.org_id, chosen.id, chosen.name);
              }}
            >
              {stopped ? "Start and Close" : "Attach and Close"}
            </Button>
          </DialogFooter>
        </div>
      </DialogContent>
    </Dialog>
  );
}
