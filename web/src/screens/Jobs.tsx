import { Briefcase, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { Body, Button, DataTable, ObjectHeader, toast, type Column } from "@/design";
import * as api from "@/api";
import { NewJob } from "@/components/NewJob";
import { StateBadge, span, who } from "@/components/RunBits";
import { when } from "@/format";
import { useLoad } from "@/hooks";
import { go } from "@/route";

/** Python scripts that ran over the workspace's data, each in a sandbox of its own: newest first. */
export function Jobs({ ws }: { ws: string }) {
  const [creating, setCreating] = useState(false);
  const [filter, setFilter] = useState("");
  const runs = useLoad(() => api.runs(ws, { kind: "job", limit: 100 }), ws);
  const computes = useLoad(() => api.computeTargets(ws), ws);
  const all = runs.data ?? [];
  const rows = all.filter((r) => r.title.toLowerCase().includes(filter.trim().toLowerCase()));
  const live = all.some(api.isRunning);
  const { reload } = runs;
  useEffect(() => {
    if (!live) return;
    const t = setInterval(reload, 2500);
    return () => clearInterval(t);
  }, [live, reload]);
  const columns: Column<api.Run>[] = [
    { key: "state", header: "Status", cell: (r) => <StateBadge state={r.state} /> },
    { key: "title", header: "Job", width: "fill", cell: (r) => <span className="font-medium text-accent">{r.title}</span> },
    { key: "by", header: "By", cell: (r) => who(r) },
    { key: "on", header: "Sandbox on", cell: (r) => computes.data?.find((c) => c.id === r.target_id)?.name ?? "removed" },
    { key: "at", header: "Started", cell: (r) => when(r.created_ms) },
    { key: "took", header: "Duration", align: "right", cell: (r) => (r.run_ms >= 0 ? span(r.run_ms) : "") },
  ];
  return (
    <Body>
      <ObjectHeader
        icon={Briefcase}
        title="Jobs"
        actions={
          <Button variant="primary" onClick={() => setCreating(true)}>
            <Plus /> New job
          </Button>
        }
      />
      <DataTable columns={columns} rows={rows} rowKey={(r) => r.id} onOpen={(r) => go({ screen: "run", ws, id: r.id })} filter={filter} onFilter={setFilter} count={runs.loading && !runs.data ? "Loading…" : `${rows.length} ${rows.length === 1 ? "run" : "runs"}`} empty={runs.error?.message ?? (filter ? "No matching jobs." : "No jobs yet. A job runs a Python script over tables and volumes you choose, and saves what it writes.")} />
      <NewJob
        ws={ws}
        open={creating}
        onOpenChange={setCreating}
        onStarted={(r) => {
          toast.success("Job started");
          go({ screen: "run", ws, id: r.id });
        }}
      />
    </Body>
  );
}
