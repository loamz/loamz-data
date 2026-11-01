import { Cpu, MoreHorizontal, Plus, Server, TerminalSquare, Trash2, Zap } from "lucide-react";
import { useEffect, useState } from "react";
import { Badge, Body, Button, DataTable, Menu, MenuContent, MenuItem, MenuTrigger, ObjectHeader, Tabs, TabsContent, TabsList, TabsTrigger, confirmAction, toast, type Column } from "@/design";
import * as api from "@/api";
import { CreateCompute } from "@/components/CreateCompute";
import { PowerButton } from "@/components/PowerButton";
import { computeState, sizeLabel, stateLabel, type ComputeState } from "@/computeState";
import { go } from "@/route";
import { when } from "@/format";
import { useLoad } from "@/hooks";

/** The compute work runs on: serverless SQL warehouses that start when a statement arrives, and sandboxes on machines of your own. */
export function Compute({ ws, me }: { ws: string; me: api.Me | undefined }) {
  const info = me?.workspaces.find((w) => w.id === ws);
  const [making, setMaking] = useState(false);
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

  const columns = (kind: api.Compute["kind"]): Column<api.Compute>[] => [
    {
      key: "state",
      header: "State",
      cell: (c) => {
        const s: ComputeState = computeState(c, now);
        return <Badge tone={s === "running" ? "success" : s === "starting" ? "accent" : "neutral"}>{stateLabel[s]}</Badge>;
      },
    },
    { key: "name", header: "Name", width: "fill", cell: (c) => <span className="inline-flex items-center gap-2">{c.kind === "serverless" ? <Zap className="text-muted" aria-hidden /> : <Server className="text-muted" aria-hidden />}<span className="font-medium">{c.name}</span></span> },
    { key: "size", header: "Size", cell: (c) => sizeLabel(c) },
    { key: "used", header: "Last used", cell: (c) => (c.last_used_ms ? when(c.last_used_ms) : "never") },
    {
      key: "act",
      header: "",
      cell: (c) => (
        <span className="flex items-center justify-end gap-1" onClick={(e) => e.stopPropagation()}>
          {info?.admin && kind === "serverless" && <PowerButton org={info.org_id} compute={c} state={computeState(c, now)} onChanged={reload} />}
          {kind === "serverless" && (
            <Button variant="secondary" size="sm" onClick={() => go({ screen: "sql", ws })}>
              <TerminalSquare /> Query
            </Button>
          )}
          {info?.admin && (
            <Menu>
              <MenuTrigger asChild>
                <Button variant="ghost" size="icon-sm" aria-label={`More for ${c.name}`}>
                  <MoreHorizontal />
                </Button>
              </MenuTrigger>
              <MenuContent align="end">
                <MenuItem
                  className="text-danger"
                  onSelect={async () => {
                    if (!(await confirmAction({ title: `Delete ${c.name}?`, description: "What is waiting or running on it fails.", confirmLabel: "Delete", danger: true }))) return;
                    try {
                      await api.detachCompute(info.org_id, c.id);
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
  const warehouses = all.filter((c) => c.kind === "serverless");
  const sandboxes = all.filter((c) => c.kind === "agent");
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
      <Tabs defaultValue="warehouses" className="flex flex-col gap-4">
        <TabsList variant="line">
          <TabsTrigger value="warehouses">SQL warehouses</TabsTrigger>
          <TabsTrigger value="sandboxes">Sandboxes</TabsTrigger>
        </TabsList>
        <TabsContent value="warehouses">
          <DataTable columns={columns("serverless")} rows={warehouses} rowKey={(c) => c.id} count={`${warehouses.length} ${warehouses.length === 1 ? "warehouse" : "warehouses"}`} empty="No SQL warehouse yet. A serverless one starts when a statement arrives and stops a minute after." />
        </TabsContent>
        <TabsContent value="sandboxes">
          <DataTable columns={columns("agent")} rows={sandboxes} rowKey={(c) => c.id} count={`${sandboxes.length} ${sandboxes.length === 1 ? "machine" : "machines"}`} empty="No machine attached. Jobs and agent code run in a gVisor sandbox on a machine you attach." />
        </TabsContent>
      </Tabs>
      {info && <CreateCompute org={info.org_id} open={making} onOpenChange={setMaking} onCreated={reload} />}
    </Body>
  );
}
