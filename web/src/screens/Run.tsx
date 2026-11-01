import { Briefcase, Database } from "lucide-react";
import { Body, ObjectHeader, Page, PageEmpty, WithRail } from "@/design";
import * as api from "@/api";
import { RunFacts, StopButton, Text, useRun } from "@/components/RunBits";
import { useLoad } from "@/hooks";
import { go } from "@/route";

/** One run on a page of its own: how it went, what it printed, and what it was asked to do. */
export function Run({ ws, id }: { ws: string; id: string }) {
  const run = useRun(ws, id);
  const computes = useLoad(() => api.computeTargets(ws), ws);
  const r = run.data;
  if (run.error && !r) {
    return (
      <Page title="Not found" onBack={() => go({ screen: "jobs", ws })}>
        <PageEmpty icon={Database} title="This run isn't here, or you can't see it">{run.error.message}</PageEmpty>
      </Page>
    );
  }
  if (!r) return <Body><p className="text-sm text-muted">Loading…</p></Body>;
  const back = r.kind === "job" ? { label: "Jobs", to: "jobs" as const } : { label: "Query history", to: "history" as const };
  return (
    <Body>
      <ObjectHeader
        crumbs={[{ label: back.label, onClick: () => go({ screen: back.to, ws }) }]}
        icon={Briefcase}
        title={r.title}
        actions={<StopButton ws={ws} run={r} onStopped={run.reload} />}
      />
      <WithRail rail={<RunFacts run={r} computeName={(t) => computes.data?.find((c) => c.id === t)?.name ?? "removed"} />}>
        <div className="flex flex-col gap-4">
          {r.error && <p className="whitespace-pre-wrap text-sm text-danger">{r.error}</p>}
          {r.kind === "job" && r.state === "succeeded" && <p className="text-sm text-muted">Its files are in the output volume, under <span className="font-mono">runs/{r.id}/</span>.</p>}
          <Text title="Output" text={r.stdout ?? ""} />
          <Text title="Errors" text={r.stderr ?? ""} danger />
          {r.kind === "job" && <Text title="Script" text={r.spec?.script ?? ""} />}
          {r.kind === "sql" && <Text title="Statement" text={r.spec?.sql ?? ""} />}
        </div>
      </WithRail>
    </Body>
  );
}
