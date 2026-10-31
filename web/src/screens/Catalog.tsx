import { Database, MoreHorizontal, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Badge, Button, Menu, MenuContent, MenuItem, MenuTrigger, Page, PageEmpty, SettingGroup, SettingRow, Tabs, TabsContent, TabsList, TabsTrigger, confirmAction, toast } from "@/design";
import * as api from "@/api";
import { AccessList } from "@/components/AccessList";
import { FilesTab } from "@/components/FilesTab";
import { NewDialog } from "@/components/NewDialog";
import { Tree } from "@/components/Tree";
import { when } from "@/format";
import { useLoad, useWide } from "@/hooks";
import { canContain, kindLabel, shortId } from "@/kinds";

const go = (id: string | null) => {
  location.hash = id ? `#/catalog/${id}` : "#/catalog";
};

export function Catalog({ id }: { id: string | null }) {
  const [children, setChildren] = useState<Record<string, api.Securable[]>>({});
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [loaded, setLoaded] = useState(false);
  const [creating, setCreating] = useState(false);
  const selected = useLoad(() => (id ? api.get(id) : Promise.resolve(null)), id ?? "none");

  const load = useCallback(async (parent: string) => {
    try {
      const items = await api.list(parent);
      setChildren((c) => ({ ...c, [parent]: items }));
    } catch (e) {
      toast.error("Could not load the catalog", (e as Error).message);
    }
  }, []);

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
        up = (await api.get(up).catch(() => null))?.parent_id ?? "";
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

  return (
    <div className="flex h-full min-h-0">
      <aside className={`${id ? "hidden md:flex" : "flex"} w-full shrink-0 flex-col border-border bg-sidebar md:w-80 md:border-r`}>
        <div className="flex items-center gap-2 px-4 pb-2 pt-4">
          <h1 className="flex-1 text-sm font-semibold text-foreground">Catalog</h1>
          <Button variant="primary" size="sm" onClick={() => setCreating(true)}>
            <Plus /> New
          </Button>
        </div>
        <div className="scroll-area min-h-0 flex-1 px-2 pb-4">
          {topEmpty ? (
            <p className="px-3 py-6 text-center text-xs text-muted">Nothing here yet. Create a catalog to begin.</p>
          ) : (
            <Tree byParent={children} expanded={expanded} selected={id} onToggle={toggle} onOpen={(s) => go(s.id)} />
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
          <Page title="Not found" onBack={() => go(null)}>
            <PageEmpty icon={Database} title="This isn't here, or you can't see it">{selected.error.message}</PageEmpty>
          </Page>
        ) : selected.data ? (
          <Detail
            key={selected.data.id}
            item={selected.data}
            onBack={() => go(null)}
            onDropped={async (s) => {
              go(s.parent_id || null);
              await load(s.parent_id);
            }}
          />
        ) : null}
      </section>

      <NewDialog
        open={creating}
        onOpenChange={setCreating}
        parent={parentOfNew}
        where={where}
        onCreated={async (made) => {
          toast.success(`Created ${made.name}`);
          await load(made.parent_id);
          if (made.parent_id) setExpanded((e) => new Set([...e, made.parent_id]));
          go(made.id);
        }}
      />
    </div>
  );
}

function Detail({ item, onBack, onDropped }: { item: api.Securable; onBack: () => void; onDropped: (s: api.Securable) => void }) {
  const wide = useWide();
  const trail = useLoad(async () => {
    const names: string[] = [];
    let up = item.parent_id;
    while (up) {
      const p = await api.get(up).catch(() => null);
      if (!p) break;
      names.unshift(p.name);
      up = p.parent_id;
    }
    return names;
  }, item.id);

  const subtitle = [...(trail.data ?? []), kindLabel[item.kind].toLowerCase()].join(" / ");
  return (
    <Page
      title={item.name}
      subtitle={subtitle}
      onBack={wide ? undefined : onBack}
      actions={
        <Menu>
          <MenuTrigger asChild>
            <Button variant="ghost" size="icon" aria-label="More">
              <MoreHorizontal />
            </Button>
          </MenuTrigger>
          <MenuContent align="end">
            <MenuItem
              className="text-danger"
              onSelect={async () => {
                if (!(await confirmAction({ title: `Drop ${item.name}?`, description: `${item.name} and everything inside it disappear now. Its files are removed for good in the background.`, confirmLabel: "Drop", danger: true }))) return;
                try {
                  await api.drop(item.id);
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
      }
    >
      <Tabs defaultValue={item.kind === "volume" ? "files" : "about"} className="space-y-4">
        <TabsList>
          {item.kind === "volume" && <TabsTrigger value="files">Files</TabsTrigger>}
          <TabsTrigger value="about">About</TabsTrigger>
          <TabsTrigger value="access">Access</TabsTrigger>
        </TabsList>
        {item.kind === "volume" && (
          <TabsContent value="files">
            <FilesTab volume={item.id} />
          </TabsContent>
        )}
        <TabsContent value="about">
          <SettingGroup title="Details">
            <SettingRow label="Owner" description="The one who made it. They manage it.">
              <span className="font-mono text-xs" title={item.owner}>{shortId(item.owner)}</span>
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
          <AccessList id={item.id} />
        </TabsContent>
      </Tabs>
    </Page>
  );
}
