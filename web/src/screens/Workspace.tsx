import { Database } from "lucide-react";
import { Body, DataTable, ObjectHeader, Page, PageEmpty, type Column } from "@/design";
import * as api from "@/api";
import { agentsUrl } from "@/agents";
import { Ask } from "@/components/Ask";
import { Principal } from "@/components/Principal";
import { when } from "@/format";
import { useLoad } from "@/hooks";
import { kindIcon, kindLabel } from "@/kinds";
import { recents, type Recent } from "@/recents";
import { go } from "@/route";

/** The front door of one workspace: what you opened lately, and the catalogs you can reach. */
export function Workspace({ ws }: { ws: string; me: api.Me | undefined }) {
  const w = useLoad(() => api.workspace(ws), ws);
  const catalogs = useLoad(() => api.list(ws, ""), ws);
  if (w.error) {
    return (
      <Page title="Workspace not found">
        <PageEmpty icon={Database} title="This workspace isn't here, or you are not part of it">
          Check the address.
        </PageEmpty>
      </Page>
    );
  }
  const lately = recents(ws);
  const open = (id: string) => go({ screen: "catalog", ws, id });
  const recentColumns: Column<Recent>[] = [
    { key: "name", header: "Name", width: "fill", cell: (r) => <Named kind={r.kind} name={r.name} /> },
    { key: "kind", header: "Type", cell: (r) => kindLabel[r.kind] },
    { key: "at", header: "Opened", cell: (r) => when(r.at) },
  ];
  const catalogColumns: Column<api.Securable>[] = [
    { key: "name", header: "Name", width: "fill", cell: (c) => <Named kind="catalog" name={c.name} /> },
    { key: "owner", header: "Owner", cell: (c) => <Principal id={c.owner} /> },
    { key: "created", header: "Created", cell: (c) => when(c.created_ms) },
  ];
  return (
    <Body>
      <ObjectHeader title={w.data?.name ?? "Workspace"} />
      {agentsUrl && <Ask base={agentsUrl} ws={ws} catalogs={(catalogs.data ?? []).map((c) => c.name)} />}
      {lately.length > 0 && (
        <section className="flex flex-col gap-2">
          <h2 className="text-sm font-semibold">Recents</h2>
          <DataTable columns={recentColumns} rows={lately} rowKey={(r) => r.id} onOpen={(r) => open(r.id)} />
        </section>
      )}
      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-semibold">Catalogs</h2>
        <DataTable columns={catalogColumns} rows={catalogs.data ?? []} rowKey={(c) => c.id} onOpen={(c) => open(c.id)} empty="No catalogs yet. Make one in the Catalog." />
      </section>
    </Body>
  );
}

export function Named({ kind, name }: { kind: api.Kind; name: string }) {
  const Icon = kindIcon[kind];
  return (
    <span className="inline-flex min-w-0 items-center gap-2">
      <Icon className="shrink-0 text-muted" aria-hidden />
      <span className="truncate font-medium text-accent">{name}</span>
    </span>
  );
}
