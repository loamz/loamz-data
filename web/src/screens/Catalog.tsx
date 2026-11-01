import { Database, MoreHorizontal, Plus, TerminalSquare, Trash2, UserCog } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Badge, Button, Dialog, DialogContent, DialogFooter, Menu, MenuContent, MenuItem, MenuTrigger, Page, PageEmpty, SettingGroup, SettingRow, Tabs, TabsContent, TabsList, TabsTrigger, confirmAction, toast } from "@/design";
import * as api from "@/api";
import { go } from "@/route";
import { openInEditor } from "@/sqlTabs";
import { AccessList } from "@/components/AccessList";
import { FilesTab } from "@/components/FilesTab";
import { SampleData, TableTab } from "@/components/TableTab";
import { NewDialog } from "@/components/NewDialog";
import { PrincipalField } from "@/components/PrincipalField";
import { Tree } from "@/components/Tree";
import { WorkspaceSwitcher } from "@/components/WorkspaceSwitcher";
import { when } from "@/format";
import { useLoad, useWide } from "@/hooks";
import { Principal } from "@/components/Principal";
import { canContain, kindLabel } from "@/kinds";

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

  if (here.error) {
    return (
      <Page title="Workspace not found">
        <PageEmpty icon={Database} title="This workspace isn't here, or you are not part of it">
          Check the address.
        </PageEmpty>
      </Page>
    );
  }

  return (
    <div className="flex h-full min-h-0">
      <aside className={`${id ? "hidden md:flex" : "flex"} w-full shrink-0 flex-col border-border bg-sidebar md:w-80 md:border-r`}>
        <div className="flex items-center gap-2 px-4 pb-2 pt-4">
          <div className="min-w-0 flex-1">{me ? <WorkspaceSwitcher current={ws} all={me.workspaces} /> : <h1 className="text-sm font-semibold text-foreground">Catalog</h1>}</div>
          <Button variant="primary" size="sm" onClick={() => setCreating(true)}>
            <Plus /> New
          </Button>
        </div>
        <div className="scroll-area min-h-0 flex-1 px-2 pb-4">
          {topEmpty ? (
            <p className="px-3 py-6 text-center text-xs text-muted">Nothing here yet. Create a catalog to begin.</p>
          ) : (
            <Tree byParent={children} expanded={expanded} selected={id} onToggle={toggle} onOpen={(s) => open(s.id)} />
          )}
        </div>
      </aside>

      <section className={`${id ? "block" : "hidden md:block"} min-w-0 flex-1`}>
        {!id ? (
          <Page title="Catalog" subtitle="Everything you keep here: catalogs hold schemas, and schemas hold tables, volumes and views.">
            <PageEmpty icon={Database} title={topEmpty ? "Start with a catalog" : "Pick something on the left"} action={topEmpty ? <Button variant="primary" onClick={() => setCreating(true)}><Plus /> New catalog</Button> : undefined}>
              {topEmpty ? "A catalog is the top-level home for a team's data." : "Open a volume to see its files, or any item to see who can reach it."}
            </PageEmpty>
          </Page>
        ) : selected.error ? (
          <Page title="Not found" onBack={() => open(null)}>
            <PageEmpty icon={Database} title="This isn't here, or you can't see it">{selected.error.message}</PageEmpty>
          </Page>
        ) : selected.data ? (
          <Detail
            key={selected.data.id}
            ws={ws}
            item={selected.data}
            onBack={() => open(null)}
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

function Detail({ ws, item, onBack, onDropped, onChanged }: { ws: string; item: api.Securable; onBack: () => void; onDropped: (s: api.Securable) => void; onChanged: () => void }) {
  const wide = useWide();
  const [handing, setHanding] = useState(false);
  const [heir, setHeir] = useState("");
  const trail = useLoad(async () => {
    const names: string[] = [];
    let up = item.parent_id;
    while (up) {
      const p = await api.get(ws, up).catch(() => null);
      if (!p) break;
      names.unshift(p.name);
      up = p.parent_id;
    }
    return names;
  }, item.id);

  const subtitle = [...(trail.data ?? []), kindLabel[item.kind].toLowerCase()].join(" / ");
  const isVolume = item.kind === "volume";
  const isTable = item.kind === "table";
  // A table is named in full in SQL: catalog.schema.table.
  const fullName = trail.data?.length === 2 ? `${trail.data.join(".")}.${item.name}` : null;
  return (
    <Page
      title={item.name}
      subtitle={subtitle}
      onBack={wide ? undefined : onBack}
      actions={
        <>
          {item.kind === "table" && fullName && (
            <Button variant="primary" onClick={() => openInEditor(ws, `SELECT * FROM ${fullName} LIMIT 100`, item.name)}>
              <TerminalSquare /> Query
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
    >
      <Tabs defaultValue={isVolume ? "files" : isTable ? "schema" : "about"} className="space-y-4">
        <TabsList>
          {isVolume && <TabsTrigger value="files">Files</TabsTrigger>}
          {isTable && <TabsTrigger value="schema">Table</TabsTrigger>}
          {isTable && <TabsTrigger value="sample">Sample data</TabsTrigger>}
          <TabsTrigger value="about">About</TabsTrigger>
          <TabsTrigger value="access">Access</TabsTrigger>
        </TabsList>
        {isVolume && (
          <TabsContent value="files">
            <FilesTab ws={ws} id={item.id} />
          </TabsContent>
        )}
        {isTable && (
          <TabsContent value="schema">
            <TableTab ws={ws} id={item.id} catalog={trail.data?.[0] ?? ""} full={fullName} />
          </TabsContent>
        )}
        {isTable && (
          <TabsContent value="sample">
            <SampleData ws={ws} full={fullName} />
          </TabsContent>
        )}
        <TabsContent value="about">
          <SettingGroup title="Details">
            <SettingRow label="Owner" description="The one who made it. They manage it.">
              <Principal id={item.owner} />
            </SettingRow>
            <SettingRow label="Classification" description="How carefully the data is handled.">
              <Badge tone={item.classification === "normal" ? "neutral" : "warning"}>{item.classification}</Badge>
            </SettingRow>
            <SettingRow label="Placement" description="Where the bytes live.">
              <Badge>{item.placement}</Badge>
            </SettingRow>
            <SettingRow label="Consistency" description={item.consistency === "transaction" ? "Changes happen all together or not at all." : "Each change appears whole; one writer at a time."}>
              <Badge>{item.consistency}</Badge>
            </SettingRow>
            <SettingRow label="Created">
              <span className="text-xs text-muted">{when(item.created_ms)}</span>
            </SettingRow>
          </SettingGroup>
        </TabsContent>
        <TabsContent value="access">
          <AccessList ws={ws} id={item.id} />
        </TabsContent>
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
    </Page>
  );
}
