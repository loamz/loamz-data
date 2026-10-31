import { Briefcase, ChevronRight, Cpu, Database, Settings as Gear, TerminalSquare } from "lucide-react";
import { Badge, Page, PageEmpty, Section } from "@/design";
import * as api from "@/api";
import { WorkspaceSwitcher } from "@/components/WorkspaceSwitcher";
import { useLoad } from "@/hooks";
import { go } from "@/route";

/** The front door of one workspace: what is in it and what you can do here. Everything else in a workspace starts from this page. */
export function Workspace({ ws, me }: { ws: string; me: api.Me | undefined }) {
  const w = useLoad(() => api.workspace(ws), ws);
  const catalogs = useLoad(() => api.list(ws, ""), ws);
  if (w.error) {
    return (
      <Page title="Workspace not found" onBack={() => go("/")}>
        <PageEmpty icon={Database} title="This workspace isn't here, or you are not part of it">
          Go back Home to see the workspaces you can open.
        </PageEmpty>
      </Page>
    );
  }
  const here = w.data;
  if (!here) return <Page title="Workspace">{null}</Page>;
  const org = me?.workspaces.find((x) => x.id === ws)?.org_alias;
  const items = catalogs.data ?? [];
  return (
    <Page
      title={here.name}
      subtitle={`${org === "Personal" ? "Your own space" : (org ?? "")} · you are ${here.account_admin ? "an account admin" : here.admin ? "an admin" : "a user"} here`}
      actions={me && <WorkspaceSwitcher current={ws} all={me.workspaces} where="overview" />}
      layout="columns"
    >
      <Section title="Catalogs" description="The data you can reach in this workspace. Open one to browse its schemas, tables and files.">
        <div className="-m-1">
          {items.length === 0 && !catalogs.loading ? (
            <button type="button" onClick={() => go({ screen: "catalog", ws, id: null })} className="flex w-full cursor-pointer items-center gap-3 rounded-lg px-3 py-3 text-left text-sm text-muted transition-colors hover:bg-card-hover">
              <Database aria-hidden /> <span className="flex-1">Nothing here yet. Open the catalog to create the first one.</span>
              <ChevronRight aria-hidden />
            </button>
          ) : (
            <div className="divide-y divide-border">
              {items.map((c) => (
                <button key={c.id} type="button" onClick={() => go({ screen: "catalog", ws, id: c.id })} className="flex w-full cursor-pointer items-center gap-3 rounded-lg px-3 py-3 text-left transition-colors hover:bg-card-hover">
                  <Database className="shrink-0 text-muted" aria-hidden />
                  <span className="min-w-0 flex-1 truncate text-sm font-medium text-foreground">{c.name}</span>
                  {c.classification !== "normal" && <Badge tone="warning">{c.classification}</Badge>}
                  <ChevronRight className="shrink-0 text-muted" aria-hidden />
                </button>
              ))}
              <button type="button" onClick={() => go({ screen: "catalog", ws, id: null })} className="flex w-full cursor-pointer items-center gap-3 rounded-lg px-3 py-3 text-left text-sm text-muted transition-colors hover:bg-card-hover">
                <span className="flex-1">Open the catalog explorer</span>
                <ChevronRight aria-hidden />
              </button>
            </div>
          )}
        </div>
      </Section>

      <Section title="Run and query" description="Work that runs on compute attached to this workspace. Not available yet; they will appear here, with the same access rules as the data.">
        <div className="-m-1 divide-y divide-border">
          {[
            { icon: Briefcase, name: "Jobs and ETL", note: "Daily pipelines, run history" },
            { icon: TerminalSquare, name: "SQL editor", note: "Ad-hoc queries on a serverless warehouse" },
            { icon: Cpu, name: "Compute", note: "Warehouses, job compute and agent sandboxes" },
          ].map(({ icon: Icon, name, note }) => (
            <div key={name} className="flex items-center gap-3 px-3 py-3">
              <Icon className="shrink-0 text-muted" aria-hidden />
              <span className="min-w-0 flex-1">
                <span className="block text-sm font-medium text-foreground">{name}</span>
                <span className="block text-xs text-muted">{note}</span>
              </span>
              <Badge>On the way</Badge>
            </div>
          ))}
        </div>
      </Section>

      {here.admin && (
        <Section title="Manage this workspace" description="Who works here, what they may do, and which catalogs show here.">
          <button type="button" onClick={() => go({ screen: "settings", ws })} className="-m-1 flex w-[calc(100%+0.5rem)] cursor-pointer items-center gap-3 rounded-lg px-3 py-3 text-left transition-colors hover:bg-card-hover">
            <Gear className="shrink-0 text-muted" aria-hidden />
            <span className="flex-1 text-sm font-medium text-foreground">Members, entitlements and catalogs</span>
            <ChevronRight className="shrink-0 text-muted" aria-hidden />
          </button>
        </Section>
      )}
    </Page>
  );
}
