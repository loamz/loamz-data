import { Loader2 } from "lucide-react";
import { cn } from "@/design";
import { stateLabel, type ComputeState } from "@/computeState";

/** A compute's state as a mark: green when running, grey when stopped or offline, a spinner while it starts. */
export function StateDot({ state, className }: { state: ComputeState; className?: string }) {
  if (state === "starting") return <Loader2 className={cn("icon-free size-3.5 shrink-0 animate-spin text-accent", className)} role="img" aria-label={stateLabel[state]} />;
  return <span role="img" aria-label={stateLabel[state]} title={stateLabel[state]} className={cn("inline-block size-2.5 shrink-0 rounded-full", state === "running" ? "bg-success" : "bg-muted/50", className)} />;
}
