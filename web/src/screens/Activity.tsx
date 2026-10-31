import { CheckCircle2, ScrollText, ShieldAlert, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { Badge, Button, Page, PageEmpty, Segmented, toast } from "@/design";
import * as api from "@/api";
import { describe, shortTarget, when } from "@/format";
import { useLoad } from "@/hooks";
import { useNames } from "@/names";
import { shortId } from "@/kinds";

export function Activity() {
  const trail = useLoad(() => api.audit(200), "audit");
  const [only, setOnly] = useState<"all" | "denied">("all");
  const [verdict, setVerdict] = useState<api.Verdict | null>(null);
  const [checking, setChecking] = useState(false);

  const all = trail.data ?? [];
  const names = useNames(all.map((e) => e.target));
  const shown = only === "denied" ? all.filter((e) => e.outcome === "denied") : all;

  return (
    <Page
      title="Activity"
      subtitle="Everything done with your data, newest first. Entries can't be changed or removed."
      actions={
        <>
          {verdict && (
            <Badge tone={verdict.intact ? "success" : "danger"}>
              {verdict.intact ? <ShieldCheck className="size-3" /> : <ShieldAlert className="size-3" />}
              {verdict.intact ? `Intact · ${verdict.entries} entries` : `Broken at entry ${verdict.first_broken}`}
            </Badge>
          )}
          <Button
            disabled={checking}
            onClick={async () => {
              setChecking(true);
              try {
                setVerdict(await api.verify());
              } catch (e) {
                toast.error("Could not check the record", (e as Error).message);
              } finally {
                setChecking(false);
              }
            }}
          >
            <CheckCircle2 /> {checking ? "Checking…" : "Check the record"}
          </Button>
        </>
      }
    >
      {trail.error && !trail.data ? (
        <PageEmpty icon={ScrollText} title="Only managers can read the activity">
          {trail.error.status === 403 || trail.error.status === 404 ? "Ask an owner of this space for the Owner role." : trail.error.message}
        </PageEmpty>
      ) : (
        <>
          <div className="flex items-center justify-between gap-2">
            <p className="text-xs text-muted">{trail.loading && !trail.data ? "Loading…" : `Showing the latest ${all.length}.`}</p>
            <Segmented
              label="Show"
              value={only}
              onChange={setOnly}
              options={[
                { value: "all", label: "All" },
                { value: "denied", label: "Refused" },
              ]}
            />
          </div>
          {shown.length === 0 && !trail.loading ? (
            <PageEmpty icon={ScrollText} title={only === "denied" ? "Nothing was refused" : "No activity yet"}>
              {only === "denied" ? "Every request in this list was allowed." : "Things appear here as soon as someone does something."}
            </PageEmpty>
          ) : (
            <div className="divide-y divide-border rounded-xl border border-border bg-background/40">
              {shown.map((e) => (
                <div key={e.seq} className="flex items-center gap-3 px-4 py-2.5">
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm text-foreground">
                      <span className="font-medium" title={e.actor}>{shortId(e.actor)}</span>
                      {e.on_behalf_of && <span className="text-muted"> for <span title={e.on_behalf_of}>{shortId(e.on_behalf_of)}</span></span>}
                      <span className="text-muted"> · </span>
                      {describe(e.action, e.outcome)}
                    </p>
                    <p className="truncate text-xs text-muted" title={e.target}>{shortTarget(e.target, names)}</p>
                  </div>
                  {e.outcome !== "ok" && <Badge tone="danger">{e.outcome === "denied" ? "Refused" : e.outcome}</Badge>}
                  <span className="w-20 shrink-0 text-right text-xs tabular-nums text-muted">{when(e.at_ms)}</span>
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </Page>
  );
}
