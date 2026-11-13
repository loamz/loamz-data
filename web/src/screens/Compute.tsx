import { Cpu, KeyRound, MoreHorizontal, Plus, Server, TerminalSquare, Trash2, Zap } from "lucide-react";
import { useEffect, useState } from "react";
import { Body, Button, DataTable, Dialog, DialogContent, Menu, MenuContent, MenuItem, MenuTrigger, ObjectHeader, confirmAction, toast, type Column } from "@/design";
import * as api from "@/api";
import { AgentCommand } from "@/components/AgentCommand";
import { CreateCompute } from "@/components/CreateCompute";
import { StateDot } from "@/components/StateDot";
import { PowerButton } from "@/components/PowerButton";
import { computeState, sizeLabel, stateLabel, type ComputeState } from "@/computeState";
import { go } from "@/route";
import { when } from "@/format";
import { useLoad } from "@/hooks";

/**
 * The compute work runs on, in one list. What differs is where it runs, not what it is for: a serverless one runs SQL, starts when you start it and
 * stops a minute after its last statement; a machine of your own is on while its agent is connected and runs SQL and Python jobs in a gVisor sandbox.
 */
export function Compute({ ws, me }: { ws: string; me: api.Me | undefined }) {
  const info = me?.workspaces.find((w) => w.id === ws);
  const [making, setMaking] = useState(false);
  const [shown, setShown] = useState<api.Compute | null>(null);
  const [filter, setFilter] = useState("");
  const [now, setNow] = useState(() => Date.now());
  const targets = useLoad(() => api.computeTargets(ws), ws);
  const { reload } = targets;
  const all = targets.data ?? [];
  // A warehouse notices it has gone idle, and a starting one is watched until it is up.
  const busy = all.some((c) => c.starting);
  useEffect(() => {
    const t = setInterval(() => {
      setNow(Date.now());
      if (busy) reload();
    }, busy ? 2000 : 5000);
    return () => clearInterval(t);
  }, [busy, reload]);

  const columns: Column<api.Compute>[] = [
    {
      key: "state",
      header: "State",
      cell: (c) => {
        const s: ComputeState = computeState(c, now);
        return <span className="inline-flex items-center gap-2"><StateDot state={s} />{stateLabel[s]}</span>;
      },
    },
    { key: "name", header: "Name", width: "fill", cell: (c) => <span className="inline-flex items-center gap-2">{c.kind === "serverless" ? <Zap className="text-muted" aria-hidden /> : <Server className="text-muted" aria-hidden />}<span className="font-medium">{c.name}</span></span> },
    { key: "type", header: "Type", cell: (c) => (c.kind === "serverless" ? "Serverless" : "Machine") },
    { key: "runs", header: "Runs", cell: (c) => (c.kind === "serverless" ? "SQL" : "SQL and jobs") },
    { key: "size", header: "Size", cell: (c) => sizeLabel(c) },
    { key: "used", header: "Last used", cell: (c) => (c.last_used_ms ? when(c.last_used_ms) : "never") },
    {
      key: "act",
      header: "",
      cell: (c) => (
        <span className="flex items-center justify-end gap-1" onClick={(e) => e.stopPropagation()}>
          {info?.admin && c.kind === "serverless" && <PowerButton ws={ws} compute={c} state={computeState(c, now)} onChanged={reload} />}
          <Button variant="secondary" size="sm" onClick={() => go({ screen: "sql", ws })}>
            <TerminalSquare /> Query
          </Button>
          {info?.admin && (
            <Menu>
              <MenuTrigger asChild>
                <Button variant="ghost" size="icon-sm" aria-label={`More for ${c.name}`}>
                  <MoreHorizontal />
                </Button>
              </MenuTrigger>
              <MenuContent align="end">
                {c.kind === "agent" && (
                  <MenuItem
                    onSelect={async () => {
                      if (!(await confirmAction({ title: `Give ${c.name} a new secret?`, description: "The agent using the old one is turned away until it is given the new one.", confirmLabel: "New secret" }))) return;
                      try {
                        setShown(await api.newComputeSecret(ws, c.id));
                        reload();
                      } catch (e) {
                        toast.error("Could not make a new secret", (e as Error).message);
                      }
                    }}
                  >
                    <KeyRound /> New secret…
                  </MenuItem>
                )}
                <MenuItem
                  className="text-danger"
                  onSelect={async () => {
                    if (!(await confirmAction({ title: `Delete ${c.name}?`, description: "What is waiting or running on it fails.", confirmLabel: "Delete", danger: true }))) return;
                    try {
                      await api.detachCompute(ws, c.id);
                      reload();
                    } catch (e) {
                      toast.error("Could not delete it", (e as Error).message);
                    }
                  }}
                >
                  <Trash2 /> Delete…
                </MenuItem>
              </MenuContent>
            </Menu>
          )}
        </span>
      ),
    },
  ];
  const matching = all.filter((c) => c.name.toLowerCase().includes(filter.trim().toLowerCase()));
  return (
    <Body>
      <ObjectHeader
        icon={Cpu}
        title="Compute"
        actions={
          info?.admin && (
            <Button variant="primary" onClick={() => setMaking(true)}>
              <Plus /> Create compute
            </Button>
          )
        }
      />
      <DataTable
        columns={columns}
        rows={matching}
        rowKey={(c) => c.id}
        filter={filter}
        onFilter={setFilter}
        count={`${matching.length} compute ${matching.length === 1 ? "resource" : "resources"}`}
        empty={targets.error?.message ?? (filter ? "No matching compute." : "No compute yet. A serverless one runs SQL when you start it; a machine you attach also runs Python jobs in a gVisor sandbox.")}
      />
      {info && <CreateCompute ws={ws} open={making} onOpenChange={setMaking} onCreated={reload} />}
      <Dialog open={shown !== null} onOpenChange={(o) => !o && setShown(null)}>
        <DialogContent title={`Start the agent on ${shown?.name ?? ""}`} description="This secret is shown once. Keep it in a file only the agent's user can read." className="max-w-xl">
          {shown?.secret && <AgentCommand secret={shown.secret} />}
        </DialogContent>
      </Dialog>
    </Body>
  );
}
