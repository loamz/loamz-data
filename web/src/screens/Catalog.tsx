import { Copy, Database, RefreshCw, MoreHorizontal, Plus, TerminalSquare, Trash2, Upload, UserCog } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Body, Button, DataTable, DetailBlock, DetailRow, Dialog, DialogContent, DialogFooter, Menu, MenuContent, MenuItem, MenuTrigger, ObjectHeader, Page, PageEmpty, Tabs, TabsContent, TabsList, TabsTrigger, TreePane, WithRail, confirmAction, toast, type Column } from "@/design";
import * as api from "@/api";
import { go } from "@/route";
import { openInEditor } from "@/sqlTabs";
import { AccessList } from "@/components/AccessList";
import { CatalogCompute } from "@/components/CatalogCompute";
import { TableRules } from "@/components/TableRules";
import { FilesTab } from "@/components/FilesTab";
import { SampleData, TableHistory, TableOverview } from "@/components/TableTab";
import { ImportDialog } from "@/components/ImportDialog";
import { NewDialog } from "@/components/NewDialog";
import { PrincipalField } from "@/components/PrincipalField";
import { Tree, TreeRow } from "@/components/Tree";
import { when } from "@/format";
import { useLoad } from "@/hooks";
import { Principal } from "@/components/Principal";
import { Named } from "@/screens/Workspace";
import { canContain, kindIcon, kindLabel } from "@/kinds";
import { remember } from "@/recents";

export function Catalog({ ws, id, me }: { ws: string; id: string | null; me: api.Me | undefined }) {
  const open = (to: string | null) => go({ screen: "catalog", ws, id: to });
  // An address for a workspace you are not part of says so, instead of showing an empty catalog.
  const here = useLoad(() => api.workspace(ws), ws);
  const [children, setChildren] = useState<Record<string, api.Securable[]>>({});
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [loaded, setLoaded] = useState(false);
  const [creating, setCreating] = useState(false);
  const selected = useLoad(() => (id ? api.get(ws, id) : Promise.resolve(null)), `${ws}/${id ?? "none"}`);

  const load = useCallback(async (parent: string) => {
    try {
      const items = await api.list(ws, parent);
      setChildren((c) => ({ ...c, [parent]: items }));
    } catch (e) {
      toast.error("Could not load the catalog", (e as Error).message);
    }
  }, [ws]);

  useEffect(() => {
    void load("").then(() => setLoaded(true));
  }, [load]);

  // Opening an address deep in the tree opens its parents too.
  useEffect(() => {
    const chain = selected.data;
    if (!chain || !chain.parent_id) return;
    let cancelled = false;
    void (async () => {
      let up = chain.parent_id;
      const open: string[] = [];
      while (up && !cancelled) {
        open.push(up);
        await load(up);
        up = (await api.get(ws, up).catch(() => null))?.parent_id ?? "";
      }
      if (!cancelled) setExpanded((e) => new Set([...e, ...open]));
    })();
    return () => {
      cancelled = true;
    };
  }, [selected.data?.id, load]); // eslint-disable-line react-hooks/exhaustive-deps

  const toggle = (s: api.Securable) => {
    setExpanded((e) => {
      const n = new Set(e);
      if (n.has(s.id)) n.delete(s.id);
      else {
        n.add(s.id);
        if (!children[s.id]) void load(s.id);
      }
      return n;
    });
  };

  const inside = selected.data && canContain(selected.data.kind) ? selected.data : null;
  const parentOfNew = inside;
  const where = parentOfNew ? `Inside ${parentOfNew.name}` : "At the top of your space";
  const topEmpty = loaded && (children[""] ?? []).length === 0;

  const flat = useMemo(() => Object.values(children).flat(), [children]);
  const [filter, setFilter] = useState("");

  if (here.error) {
    return (
      <Page title="Workspace not found">
        <PageEmpty icon={Database} title="This workspace isn't here, or you are not part of it">
          Check the address.
        </PageEmpty>
      </Page>
    );
  }

  const shown = filter.trim() ? flat.filter((s) => s.name.toLowerCase().includes(filter.trim().toLowerCase())) : null;
  const top = children[""] ?? [];

  const tree = (
    <TreePane
      title="Catalog"
      actions={
        <>
          <Button variant="ghost" size="icon-sm" aria-label="Refresh" onClick={() => void load(selected.data?.parent_id ?? "")}>
            <RefreshCw />
          </Button>
          <Button variant="ghost" size="icon-sm" aria-label="New" onClick={() => setCreating(true)}>
            <Plus />
          </Button>
        </>
      }
      filter={filter}
      onFilter={setFilter}
    >
      {topEmpty ? (
        <p className="px-3 py-6 text-center text-xs text-muted">Nothing here yet. Create a catalog to begin.</p>
      ) : shown ? (
        <ul className="flex flex-col gap-px">
          {shown.map((s) => (
            <li key={s.id}>
              <TreeRow depth={0} icon={kindIcon[s.kind]} label={s.name} active={id === s.id} onOpen={() => open(s.id)} />
            </li>
          ))}
        </ul>
      ) : (
        <Tree byParent={children} expanded={expanded} selected={id} onToggle={toggle} onOpen={(s) => open(s.id)} />
      )}
    </TreePane>
  );

  const catalogColumns: Column<api.Securable>[] = [
    { key: "name", header: "Name", width: "fill", cell: (c) => <Named kind={c.kind} name={c.name} /> },
    { key: "owner", header: "Owner", cell: (c) => <Principal id={c.owner} /> },
    { key: "created", header: "Created", cell: (c) => when(c.created_ms) },
  ];

  return (
    <div className="flex h-full min-h-0">
      <div className={`${id ? "hidden md:block" : "block"} h-full w-full md:w-auto`}>{tree}</div>
      <section className={`${id ? "block" : "hidden md:block"} min-w-0 flex-1`}>
        {!id ? (
          <Body>
            <ObjectHeader
              icon={Database}
              title="Catalogs"
              actions={
                <Button variant="primary" onClick={() => setCreating(true)}>
                  <Plus /> New catalog
                </Button>
              }
            />
            <DataTable columns={catalogColumns} rows={top} rowKey={(c) => c.id} onOpen={(c) => open(c.id)} count={`${top.length} ${top.length === 1 ? "catalog" : "catalogs"}`} empty="Start with a catalog: the top-level home for a team's data." />
          </Body>
        ) : selected.error ? (
          <Page title="Not found" onBack={() => open(null)}>
            <PageEmpty icon={Database} title="This isn't here, or you can't see it">{selected.error.message}</PageEmpty>
          </Page>
        ) : selected.data ? (
          <Detail
            key={selected.data.id}
            ws={ws}
            item={selected.data}
            children={children[selected.data.id]}
            load={load}
            onOpen={open}
            onNew={() => setCreating(true)}
            onChanged={selected.reload}
            onDropped={async (s) => {
              open(s.parent_id || null);
              await load(s.parent_id);
            }}
          />
        ) : null}
      </section>

      <NewDialog
        ws={ws}
        open={creating}
        onOpenChange={setCreating}
        parent={parentOfNew}
        where={where}
        onCreated={async (made) => {
          toast.success(`Created ${made.name}`);
          await load(made.parent_id);
          if (made.parent_id) setExpanded((e) => new Set([...e, made.parent_id]));
          open(made.id);
        }}
      />
    </div>
  );
}

function Detail({ ws, item, children, load, onOpen, onNew, onDropped, onChanged }: { ws: string; item: api.Securable; children: api.Securable[] | undefined; load: (parent: string) => Promise<void>; onOpen: (id: string | null) => void; onNew: () => void; onDropped: (s: api.Securable) => void; onChanged: () => void }) {
  const [handing, setHanding] = useState(false);
  const [heir, setHeir] = useState("");
  const [childFilter, setChildFilter] = useState("");
  const [importing, setImporting] = useState(false);
  const trail = useLoad(async () => {
    const up: { id: string; name: string }[] = [];
    let at = item.parent_id;
    while (at) {
      const p = await api.get(ws, at).catch(() => null);
      if (!p) break;
      up.unshift({ id: p.id, name: p.name });
      at = p.parent_id;
    }
    return up;
  }, item.id);
  useEffect(() => {
    remember(ws, { id: item.id, name: item.name, kind: item.kind });
    if (canContain(item.kind)) void load(item.id);
  }, [item.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const names = (trail.data ?? []).map((t) => t.name);
  const isVolume = item.kind === "volume";
  const isTable = item.kind === "table";
  const isContainer = canContain(item.kind);
  // A table is named in full in SQL: catalog.schema.table.
  const fullName = names.length === 2 ? `${names.join(".")}.${item.name}` : null;
  const kids = (children ?? []).filter((c) => c.name.includes(childFilter.trim().toLowerCase()));
  const kidColumns: Column<api.Securable>[] = [
    { key: "name", header: "Name", width: "fill", cell: (c) => <Named kind={c.kind} name={c.name} /> },
    { key: "kind", header: "Type", cell: (c) => kindLabel[c.kind] },
    { key: "owner", header: "Owner", cell: (c) => <Principal id={c.owner} /> },
    { key: "created", header: "Created", cell: (c) => when(c.created_ms) },
  ];
  const details = (
    <DetailBlock title={`About this ${kindLabel[item.kind].toLowerCase()}`}>
      <DetailRow label="Owner"><Principal id={item.owner} /></DetailRow>
      <DetailRow label="Created">{when(item.created_ms)}</DetailRow>
      <DetailRow label="Class">{item.classification}</DetailRow>
      <DetailRow label="Placement">{item.placement}</DetailRow>
      <DetailRow label="Writes">{item.consistency === "transaction" ? "all together or not at all" : "one writer at a time"}</DetailRow>
    </DetailBlock>
  );
  const crumbs = [{ label: "Catalog", onClick: () => onOpen(null) }, ...(trail.data ?? []).map((t) => ({ label: t.name, onClick: () => onOpen(t.id) }))];
  return (
    <Body>
      <ObjectHeader
        crumbs={crumbs}
        icon={kindIcon[item.kind]}
        title={item.name}
        tools={
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Copy the name"
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(fullName ?? [...names, item.name].join("."));
                toast.success("Copied");
              } catch {
                toast.error("Could not copy");
              }
            }}
          >
            <Copy />
          </Button>
        }
        actions={
          <>
            {isTable && fullName && (
              <>
                <Button variant="secondary" onClick={() => setImporting(true)}>
                  <Upload /> Import from a file
                </Button>
                <Button variant="primary" onClick={() => openInEditor(ws, `SELECT * FROM ${fullName} LIMIT 100`, item.name)}>
                  <TerminalSquare /> Query
                </Button>
              </>
            )}
            {isContainer && (
              <Button variant="primary" onClick={onNew}>
                <Plus /> New
              </Button>
            )}
            <Menu>
              <MenuTrigger asChild>
                <Button variant="ghost" size="icon" aria-label="More">
                  <MoreHorizontal />
                </Button>
              </MenuTrigger>
              <MenuContent align="end">
                <MenuItem onSelect={() => setHanding(true)}>
                  <UserCog /> Change owner…
                </MenuItem>
                <MenuItem
                  className="text-danger"
                  onSelect={async () => {
                    if (!(await confirmAction({ title: `Drop ${item.name}?`, description: `${item.name} and everything inside it disappear now. Its files are removed for good in the background.`, confirmLabel: "Drop", danger: true }))) return;
                    try {
                      await api.drop(ws, item.id);
                      toast.success(`Dropping ${item.name}`, "It's gone from view; the files are being removed.");
                      onDropped(item);
                    } catch (e) {
                      toast.error("Could not drop it", (e as Error).message);
                    }
                  }}
                >
                  <Trash2 /> Drop…
                </MenuItem>
              </MenuContent>
            </Menu>
          </>
        }
      />
      <Tabs defaultValue={isVolume ? "files" : "overview"} className="flex flex-col gap-3">
        <TabsList variant="line">
          {isVolume ? <TabsTrigger value="files">Files</TabsTrigger> : <TabsTrigger value="overview">Overview</TabsTrigger>}
          {isTable && <TabsTrigger value="sample">Sample data</TabsTrigger>}
          {!isContainer && !isTable && <TabsTrigger value="details">Details</TabsTrigger>}
          {isTable && <TabsTrigger value="details">Details</TabsTrigger>}
          <TabsTrigger value="access">Permissions</TabsTrigger>
          {item.kind === "catalog" && <TabsTrigger value="compute">Compute</TabsTrigger>}
          {isTable && <TabsTrigger value="rules">Rules</TabsTrigger>}
          {isTable && <TabsTrigger value="history">History</TabsTrigger>}
        </TabsList>
        {isVolume && (
          <TabsContent value="files">
            <FilesTab ws={ws} id={item.id} />
          </TabsContent>
        )}
        {isTable && (
          <TabsContent value="overview">
            <TableOverview ws={ws} id={item.id} catalog={names[0] ?? ""} owner={item.owner} created={item.created_ms} />
          </TabsContent>
        )}
        {isContainer && (
          <TabsContent value="overview">
            <WithRail rail={details}>
              <DataTable columns={kidColumns} rows={kids} rowKey={(c) => c.id} onOpen={(c) => onOpen(c.id)} filter={childFilter} onFilter={setChildFilter} count={`${kids.length} ${kids.length === 1 ? "item" : "items"}`} empty="Nothing inside yet." />
            </WithRail>
          </TabsContent>
        )}
        {isTable && (
          <TabsContent value="sample">
            <SampleData ws={ws} full={fullName} />
          </TabsContent>
        )}
        {(isVolume || isTable) && (
          <TabsContent value="details">
            <WithRail rail={<></>}>{<div className="max-w-md">{details}</div>}</WithRail>
          </TabsContent>
        )}
        <TabsContent value="access">
          <AccessList ws={ws} id={item.id} />
        </TabsContent>
        {item.kind === "catalog" && (
          <TabsContent value="compute">
            <CatalogCompute ws={ws} id={item.id} />
          </TabsContent>
        )}
        {isTable && (
          <TabsContent value="rules">
            <TableRules ws={ws} id={item.id} />
          </TabsContent>
        )}
        {isTable && (
          <TabsContent value="history">
            <TableHistory ws={ws} id={item.id} />
          </TabsContent>
        )}
      </Tabs>
      <Dialog open={handing} onOpenChange={setHanding}>
        <DialogContent title={`Change the owner of ${item.name}`} description="The owner manages it and everything inside it. You keep any access you were given separately.">
          <div className="mt-4 space-y-4">
            <PrincipalField onChange={setHeir} />
            <DialogFooter>
              <Button
                variant="primary"
                disabled={!heir}
                onClick={async () => {
                  try {
                    await api.transfer(ws, item.id, heir);
                    setHanding(false);
                    setHeir("");
                    toast.success("The owner was changed");
                    onChanged();
                  } catch (e) {
                    toast.error("Could not change the owner", (e as Error).message);
                  }
                }}
              >
                Change owner
              </Button>
            </DialogFooter>
          </div>
        </DialogContent>
      </Dialog>
      <ImportDialog
        ws={ws}
        open={importing}
        onOpenChange={setImporting}
        initialTarget={fullName ?? undefined}
      />
    </Body>
  );
}
