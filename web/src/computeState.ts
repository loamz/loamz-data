import type { Compute } from "@/api";

/** How long a serverless warehouse stays up after its last statement (what the endpoint is deployed with). */
export const IDLE_STOP_MS = 60_000;

export type ComputeState = "running" | "stopped" | "offline";

/** What a person sees of a piece of compute: a machine is running when its agent answers; a serverless warehouse is running for a
 *  minute after its last statement and otherwise stopped, which is fine: the next statement starts it. */
export function computeState(c: Pick<Compute, "kind" | "online" | "last_used_ms" | "stopped_ms">, now: number): ComputeState {
  if (c.kind === "serverless") return c.last_used_ms > 0 && c.stopped_ms < c.last_used_ms && now - c.last_used_ms < IDLE_STOP_MS ? "running" : "stopped";
  return c.online ? "running" : "offline";
}

export const stateLabel: Record<ComputeState, string> = { running: "Running", stopped: "Stopped", offline: "Offline" };
export const stateNote: Record<ComputeState, string> = {
  running: "",
  stopped: "Starts when a statement arrives",
  offline: "Its agent is not answering",
};

export const sizeLabel = (c: Pick<Compute, "max_memory_mb" | "max_cpu_millis">): string => {
  const cpu = c.max_cpu_millis / 1000;
  const gb = c.max_memory_mb / 1024;
  return `${cpu === 1 ? "1 CPU" : `${cpu} CPUs`} · ${Number.isInteger(gb) ? gb : gb.toFixed(1)} GB`;
};
