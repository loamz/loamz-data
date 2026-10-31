import { useEffect, useState } from "react";
import { Plus, X } from "lucide-react";
import { Button, Dialog, DialogContent, DialogFooter, Input, Select, toast } from "@/design";
import * as api from "@/api";
import { childKinds, kindLabel, validName } from "@/kinds";

const classes = [
  { value: "normal", label: "Normal" },
  { value: "personal", label: "Personal" },
  { value: "sealed", label: "Sealed" },
];
const columnTypes = ["string", "long", "int", "double", "boolean", "date", "timestamp"].map((t) => ({ value: t, label: t }));
const firstColumn = () => [{ name: "id", type: "long" }];
const classNote: Record<string, string> = {
  normal: "Ordinary data.",
  personal: "About people. Handled with care.",
  sealed: "Opaque bytes: never scanned, read or indexed.",
};

/** Make something inside `parent` (the top of your space when there is none). */
export function NewDialog({ ws, open, onOpenChange, parent, where, onCreated }: { ws: string; open: boolean; onOpenChange: (o: boolean) => void; parent: api.Securable | null; where: string; onCreated: (s: api.Securable) => void }) {
  const kinds = childKinds(parent?.kind ?? null);
  const [kind, setKind] = useState<api.Kind>(kinds[0] ?? "catalog");
  const [name, setName] = useState("");
  const [classification, setClassification] = useState("normal");
  const [busy, setBusy] = useState(false);
  const [columns, setColumns] = useState<{ name: string; type: string }[]>(firstColumn());
  useEffect(() => {
    if (open) {
      setKind(kinds[0] ?? "catalog");
      setName("");
      setClassification("normal");
      setColumns(firstColumn());
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, parent?.id]);

  const bad = name !== "" && !validName(name);
  const isTable = kind === "table";
  // A table is made with its columns: each named like a column, and no two the same.
  const columnsOk = !isTable || (columns.length > 0 && columns.every((c) => validName(c.name)) && new Set(columns.map((c) => c.name)).size === columns.length);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent title={`New ${kinds.length === 1 ? kindLabel[kinds[0]!].toLowerCase() : "item"}`} description={where}>
        <form
          className="mt-4 space-y-4"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!validName(name) || !columnsOk) return;
            setBusy(true);
            try {
              const made = isTable ? await api.get(ws, (await api.createTable(ws, { parent_id: parent?.id ?? "", name, classification, columns })).id) : await api.create(ws, { kind, parent_id: parent?.id ?? "", name, classification });
              onOpenChange(false);
              onCreated(made);
            } catch (err) {
              toast.error("Could not create it", (err as Error).message);
            } finally {
              setBusy(false);
            }
          }}
        >
          {kinds.length > 1 && (
            <label className="block space-y-1.5">
              <span className="text-xs font-medium text-foreground">Kind</span>
              <Select aria-label="Kind" value={kind} onValueChange={(v) => setKind(v as api.Kind)} options={kinds.map((k) => ({ value: k, label: kindLabel[k] }))} />
            </label>
          )}
          <label className="block space-y-1.5">
            <span className="text-xs font-medium text-foreground">Name</span>
            <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="sales_2026" spellCheck={false} aria-invalid={bad} />
            <span className={`block text-2xs ${bad ? "text-danger" : "text-muted"}`}>Lower-case letters, digits and underscores; start with a letter.</span>
          </label>
          {isTable && (
            <div className="space-y-1.5">
              <span className="text-xs font-medium text-foreground">Columns</span>
              {columns.map((c, i) => (
                <div key={i} className="flex items-center gap-2">
                  <Input aria-label={`Column ${i + 1} name`} value={c.name} onChange={(e) => setColumns(columns.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))} placeholder="column_name" spellCheck={false} aria-invalid={c.name !== "" && !validName(c.name)} />
                  <Select aria-label={`Column ${i + 1} type`} value={c.type} onValueChange={(v) => setColumns(columns.map((x, j) => (j === i ? { ...x, type: v } : x)))} options={columnTypes} />
                  <Button type="button" variant="secondary" size="icon-sm" aria-label={`Remove column ${i + 1}`} disabled={columns.length === 1} onClick={() => setColumns(columns.filter((_, j) => j !== i))}>
                    <X />
                  </Button>
                </div>
              ))}
              <Button type="button" variant="secondary" size="sm" onClick={() => setColumns([...columns, { name: "", type: "string" }])}>
                <Plus /> Add column
              </Button>
              <span className="block text-2xs text-muted">Rows are added with SQL or any Iceberg client, such as DuckDB.</span>
            </div>
          )}
          <label className="block space-y-1.5">
            <span className="text-xs font-medium text-foreground">Classification</span>
            <Select aria-label="Classification" value={classification} onValueChange={setClassification} options={classes} />
            <span className="block text-2xs text-muted">{classNote[classification]}</span>
          </label>
          <DialogFooter>
            <Button type="submit" variant="primary" disabled={busy || !validName(name) || !columnsOk}>
              Create
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
