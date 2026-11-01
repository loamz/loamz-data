import { Play, Square } from "lucide-react";
import { useState } from "react";
import { Button, toast } from "@/design";
import * as api from "@/api";
import type { ComputeState } from "@/computeState";

/** Start or stop serverless compute now. Nothing starts it for you: a stopped warehouse refuses work until someone starts it. It stops by itself a minute after its last
 *  statement, so Stop is for saving that minute. A machine has no button: it is run on the machine. */
export function PowerButton({ org, compute, state, onChanged }: { org: string; compute: api.Compute; state: ComputeState; onChanged: () => void }) {
  const [busy, setBusy] = useState(false);
  if (compute.kind !== "serverless") return null;
  const starting = state !== "running";
  const inFlight = state === "starting";
  return (
    <Button
      variant="secondary"
      size="sm"
      disabled={busy || inFlight}
      onClick={async () => {
        setBusy(true);
        try {
          await (starting ? api.startCompute(org, compute.id) : api.stopCompute(org, compute.id));
          toast.success(starting ? `${compute.name} is running` : `${compute.name} is stopping`);
          onChanged();
        } catch (e) {
          toast.error(starting ? "Could not start it" : "Could not stop it", (e as Error).message);
        } finally {
          setBusy(false);
        }
      }}
    >
      {starting ? <Play /> : <Square />} {busy || inFlight ? (starting ? "Starting…" : "Stopping…") : starting ? "Start" : "Stop"}
    </Button>
  );
}
