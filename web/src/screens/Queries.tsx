import { FileCode2, MoreHorizontal, Pencil, Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import { Body, Button, DataTable, Menu, MenuContent, MenuItem, MenuTrigger, ObjectHeader, confirmAction, toast, type Column } from "@/design";
import * as api from "@/api";
import { Principal } from "@/components/Principal";
import { when } from "@/format";
import { useLoad } from "@/hooks";
import { go } from "@/route";
import { blank, load, openSaved, save } from "@/sqlTabs";

/** The SQL people saved to come back to: your own, or everyone's for a workspace admin. */
export function Queries({ ws }: { ws: string }) {
  const list = useLoad(() => api.savedQueries(ws), ws);
  const [filter, setFilter] = useState("");
  const rows = (list.data ?? []).filter((q) => q.name.toLowerCase().includes(filter.trim().toLowerCase()));
  const columns: Column<api.SavedQuery>[] = [
    { key: "name", header: "Name", width: "fill", cell: (q) => <span className="inline-flex items-center gap-2"><FileCode2 className="text-muted" aria-hidden /><span className="font-medium text-accent">{q.name}</span></span> },
    { key: "owner", header: "Owner", cell: (q) => <Principal id={q.owner} /> },
    { key: "updated", header: "Changed", cell: (q) => when(q.updated_ms) },
    {
      key: "more",
      header: "",
      cell: (q) => (
        <Menu>
          <MenuTrigger asChild>
            <Button variant="ghost" size="icon-sm" aria-label={`More for ${q.name}`} onClick={(e) => e.stopPropagation()}>
              <MoreHorizontal />
            </Button>
          </MenuTrigger>
          <MenuContent align="end" onClick={(e) => e.stopPropagation()}>
            <MenuItem onSelect={() => openSaved(ws, q)}>
              <Pencil /> Open
            </MenuItem>
            <MenuItem
              className="text-danger"
              onSelect={async () => {
                if (!(await confirmAction({ title: `Delete ${q.name}?`, description: "The saved SQL is removed. Statements already run stay in the history.", confirmLabel: "Delete", danger: true }))) return;
                try {
                  await api.deleteQuery(ws, q.id);
                  list.reload();
                } catch (e) {
                  toast.error("Could not delete it", (e as Error).message);
                }
              }}
            >
              <Trash2 /> Delete…
            </MenuItem>
          </MenuContent>
        </Menu>
      ),
    },
  ];
  return (
    <Body>
      <ObjectHeader
        icon={FileCode2}
        title="Queries"
        actions={
          <Button
            variant="primary"
            onClick={() => {
              const state = load(ws);
              const t = blank(`Query ${state.tabs.length + 1}`);
              save(ws, { tabs: [...state.tabs, t], active: t.id });
              go({ screen: "sql", ws });
            }}
          >
            <Plus /> New query
          </Button>
        }
      />
      <DataTable columns={columns} rows={rows} rowKey={(q) => q.id} onOpen={(q) => openSaved(ws, q)} filter={filter} onFilter={setFilter} count={list.loading && !list.data ? "Loading…" : `${rows.length} ${rows.length === 1 ? "query" : "queries"}`} empty={list.error?.message ?? (filter ? "No matching queries." : "Nothing saved yet. Write SQL in the editor and press Save.")} />
    </Body>
  );
}
