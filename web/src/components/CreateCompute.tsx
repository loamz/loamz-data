import { Cpu, Server, Zap } from "lucide-react";
import { useEffect, useState } from "react";
import { Button, Dialog, DialogContent, DialogFooter, Input, Select, toast } from "@/design";
import * as api from "@/api";
import { AgentCommand } from "@/components/AgentCommand";
import { useLoad } from "@/hooks";
import { sizeLabel } from "@/computeState";

const MEMORY = [1024, 2048, 4096, 8192, 16384].map((mb) => ({ value: String(mb), label: `${mb / 1024} GB` }));
const CPU = [
  { value: "500", label: "½ CPU" },
  { value: "1000", label: "1 CPU" },
  { value: "2000", label: "2 CPUs" },
  { value: "4000", label: "4 CPUs" },
];
const icon = { serverless: Zap, machine: Server } as const;

/** Create compute: pick a kind (a serverless SQL warehouse, or a machine of your own), name it, and it is there. */
export function CreateCompute({ org, open, onOpenChange, onCreated }: { org: string; open: boolean; onOpenChange: (o: boolean) => void; onCreated: () => void }) {
  const kinds = useLoad(() => (open ? api.providers(org) : Promise.resolve([] as api.Provider[])), `${org}/${open}`);
  const [kind, setKind] = useState<api.Provider["id"]>("serverless");
  const [name, setName] = useState("");
  const [memory, setMemory] = useState("2048");
  const [cpu, setCpu] = useState("1000");
  const [busy, setBusy] = useState(false);
  const [machine, setMachine] = useState<api.Compute | null>(null);
  const list = kinds.data ?? [];
  const chosen = list.find((p) => p.id === kind);

  useEffect(() => {
    if (!open) return;
    setName("");
    setMachine(null);
    const first = (kinds.data ?? []).find((p) => p.available);
    if (first) setKind(first.id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, kinds.data]);

  if (machine?.secret) {
    return (
      <Dialog open={open} onOpenChange={(o) => { onOpenChange(o); if (!o) onCreated(); }}>
        <DialogContent title={`Start the agent on ${machine.name}`} description="This secret is shown once. Keep it in a file only the agent's user can read." className="max-w-xl">
          <AgentCommand secret={machine.secret} />
        </DialogContent>
      </Dialog>
    );
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent title="Create compute" description="Where statements and jobs run. Each run is in a sandbox of its own.">
        <form
          className="mt-4 space-y-4"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!chosen?.available || !name.trim()) return;
            setBusy(true);
            try {
              const made = await api.attachCompute(org, {
                name: name.trim(),
                kind: kind === "serverless" ? "serverless" : "agent",
                max_memory_mb: kind === "serverless" ? chosen.max_memory_mb : Number(memory),
                max_cpu_millis: kind === "serverless" ? chosen.max_cpu_millis : Number(cpu),
              });
              if (made.secret) setMachine(made);
              else {
                toast.success(`Created ${made.name}`);
                onOpenChange(false);
                onCreated();
              }
            } catch (err) {
              toast.error("Could not create it", (err as Error).message);
            } finally {
              setBusy(false);
            }
          }}
        >
          <div className="grid gap-2" role="radiogroup" aria-label="Kind of compute">
            {list.map((p) => {
              const Icon = icon[p.id] ?? Cpu;
              const on = p.id === kind;
              return (
                <button
                  key={p.id}
                  type="button"
                  role="radio"
                  aria-checked={on}
                  disabled={!p.available}
                  onClick={() => setKind(p.id)}
                  className={`flex cursor-pointer items-start gap-3 rounded-xl border px-4 py-3 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${on ? "border-selected bg-selected/10" : "border-border hover:bg-card-hover"}`}
                >
                  <Icon className="mt-0.5 shrink-0 text-muted" aria-hidden />
                  <span className="min-w-0 flex-1">
                    <span className="block text-sm font-medium text-foreground">{p.name}</span>
                    <span className="block text-xs text-muted">{p.available ? p.description : "Not set up on this server."}</span>
                  </span>
                </button>
              );
            })}
          </div>
          <label className="block space-y-1.5">
            <span className="text-xs font-medium text-foreground">Name</span>
            <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder={kind === "serverless" ? "warehouse" : "oracle"} />
          </label>
          {kind === "serverless" && chosen ? (
            <p className="text-xs text-muted">
              {sizeLabel(chosen)} per statement. Starts when you start it, and stops {chosen.idle_stop_s} seconds after its last statement. Nothing starts it for you.
            </p>
          ) : (
            <div className="grid grid-cols-2 gap-3">
              <label className="block space-y-1.5">
                <span className="text-xs font-medium text-foreground">Memory per run</span>
                <Select aria-label="Memory per run" value={memory} onValueChange={setMemory} options={MEMORY} />
              </label>
              <label className="block space-y-1.5">
                <span className="text-xs font-medium text-foreground">CPU per run</span>
                <Select aria-label="CPU per run" value={cpu} onValueChange={setCpu} options={CPU} />
              </label>
            </div>
          )}
          <DialogFooter>
            <Button type="submit" variant="primary" disabled={busy || !name.trim() || !chosen?.available}>
              Create
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
