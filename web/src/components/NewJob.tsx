import { useEffect, useState } from "react";
import { Button, Checkbox, Dialog, DialogContent, DialogFooter, Input, Select, toast } from "@/design";
import * as api from "@/api";
import { CodeEditor } from "@/components/CodeEditor";
import { kindIcon } from "@/kinds";

const TEMPLATE = `# What you chose to read is mounted read-only under /inputs/<name>.
# Whatever you write to /workspace is saved to the output volume when the job ends.
import os

print(os.listdir("/inputs"))
with open("/workspace/hello.txt", "w") as f:
    f.write("done")
`;

const TIMEOUTS = [
  { value: "60", label: "1 minute" },
  { value: "300", label: "5 minutes" },
  { value: "900", label: "15 minutes" },
  { value: "3600", label: "1 hour" },
];

interface Choice {
  id: string;
  kind: api.Kind;
  name: string;
  full: string;
}

/** Every table and volume the person can see in the workspace, with its full name. Small organizations have few; each level is one call. */
async function everything(ws: string): Promise<Choice[]> {
  const out: Choice[] = [];
  for (const c of await api.list(ws, "")) {
    for (const s of await api.list(ws, c.id).catch(() => [])) {
      for (const t of await api.list(ws, s.id).catch(() => [])) {
        if (t.kind === "table" || t.kind === "volume") out.push({ id: t.id, kind: t.kind, name: t.name, full: `${c.name}.${s.name}.${t.name}` });
      }
    }
  }
  return out;
}

/** Asks for a job: a Python script, what it may read, and the one volume it writes to. */
export function NewJob({ ws, open, onOpenChange, onStarted }: { ws: string; open: boolean; onOpenChange: (o: boolean) => void; onStarted: (r: api.Run) => void }) {
  const [choices, setChoices] = useState<Choice[] | null>(null);
  const [title, setTitle] = useState("");
  const [script, setScript] = useState(TEMPLATE);
  const [inputs, setInputs] = useState<string[]>([]);
  const [output, setOutput] = useState("");
  const [timeout, setTimeoutS] = useState("300");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setChoices(null);
    everything(ws).then(setChoices, () => setChoices([]));
  }, [open, ws]);

  const volumes = (choices ?? []).filter((c) => c.kind === "volume");
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent title="New job" description="Runs a Python script on the workspace's compute, with its own sandbox: no network, and only the data you choose." className="max-w-2xl">
        <form
          className="mt-4 space-y-4"
          onSubmit={async (e) => {
            e.preventDefault();
            setBusy(true);
            try {
              const r = await api.submitJob(ws, { title, script, inputs, output, timeout_s: Number(timeout) });
              onOpenChange(false);
              onStarted(r);
            } catch (err) {
              toast.error("Could not start the job", (err as Error).message);
            } finally {
              setBusy(false);
            }
          }}
        >
          <label className="block space-y-1.5">
            <span className="text-xs font-medium text-foreground">Name <span className="font-normal text-muted">(optional)</span></span>
            <Input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Daily orders summary" />
          </label>

          <div className="space-y-1.5">
            <span className="text-xs font-medium text-foreground">Script</span>
            <div className="h-56 overflow-hidden rounded-md border border-border bg-background/50">
              <CodeEditor value={script} onChange={setScript} language="python" />
            </div>
          </div>

          <div className="grid gap-4 sm:grid-cols-2">
            <fieldset className="space-y-1.5">
              <legend className="text-xs font-medium text-foreground">Reads <span className="font-normal text-muted">(read-only)</span></legend>
              <div className="scroll-area max-h-32 space-y-1 rounded-md border border-border p-2">
                {choices === null ? <p className="text-xs text-muted">Loading…</p> : choices.length === 0 ? <p className="text-xs text-muted">No tables or volumes yet.</p> : choices.map((c) => {
                  const Icon = kindIcon[c.kind];
                  return (
                    <Checkbox key={c.id} checked={inputs.includes(c.id)} onChange={(e) => setInputs(e.target.checked ? [...inputs, c.id] : inputs.filter((i) => i !== c.id))} label={<span className="inline-flex items-center gap-1.5"><Icon className="text-muted" aria-hidden />{c.full}</span>} />
                  );
                })}
              </div>
            </fieldset>
            <div className="space-y-4">
              <label className="block space-y-1.5">
                <span className="text-xs font-medium text-foreground">Writes to</span>
                <Select aria-label="Output volume" placeholder="Choose a volume" value={output} onValueChange={setOutput} options={volumes.map((v) => ({ value: v.id, label: v.full }))} />
                <span className="block text-2xs text-muted">Files appear under runs/&lt;run id&gt;/</span>
              </label>
              <label className="block space-y-1.5">
                <span className="text-xs font-medium text-foreground">Stop it after</span>
                <Select aria-label="Time limit" value={timeout} onValueChange={setTimeoutS} options={TIMEOUTS} />
              </label>
            </div>
          </div>

          <DialogFooter>
            <Button type="submit" variant="primary" disabled={busy || !script.trim() || !output}>
              Run job
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
