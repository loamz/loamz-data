import { Copy } from "lucide-react";
import { Button, toast } from "@/design";
import { centralOrigin } from "@/site";

/** How to start the agent of a machine, with the secret it was just given (shown once, only here). */
export function AgentCommand({ secret }: { secret: string }) {
  const command = `umask 077 && printf '%s' '${secret}' > ~/.loamz-agent.secret

LOAMZ_DATA_URL=${centralOrigin} \\
LOAMZ_AGENT_SECRET_FILE=~/.loamz-agent.secret \\
LOAMZ_RUNSC=~/.cache/loamz/runsc \\
LOAMZ_ROOTFS=~/.cache/loamz/rootfs \\
loamz-runner agent`;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command);
      toast.success("Copied");
    } catch {
      toast.error("Could not copy", "Select the text and copy it.");
    }
  };
  return (
    <div className="mt-4 space-y-3">
      <p className="text-xs text-muted">
        On the machine, in a shell (it needs <span className="font-mono">runsc</span> and the sandbox filesystem; <span className="font-mono">scripts/fetch-runsc.sh</span> and <span className="font-mono">server/runner/rootfs/build.sh</span> make them):
      </p>
      <pre className="scroll-area whitespace-pre-wrap break-all rounded-md border border-border bg-background/50 p-3 font-mono text-xs text-foreground">{command}</pre>
      <div className="flex justify-end">
        <Button variant="secondary" onClick={() => void copy()}>
          <Copy /> Copy
        </Button>
      </div>
      <p className="text-xs text-muted">When the agent connects, the machine shows as running within a few seconds.</p>
    </div>
  );
}
