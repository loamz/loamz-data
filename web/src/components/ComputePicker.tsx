import { useCallback, useEffect, useState } from "react";
import { Select } from "@/design";
import * as api from "@/api";
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
  const online = (targets.data ?? []).filter((c) => c.online);
  const all = targets.data ?? [];
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
  return { online, all, target: target || undefined, choose, loaded: targets.data !== undefined };
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
      options={all.map((c) => ({ value: c.id, label: `${c.name} · ${c.kind === "serverless" ? "serverless" : "machine"}` }))}
    />
  );
}
