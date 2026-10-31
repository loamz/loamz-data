import { useEffect, useState } from "react";
import { Button, Dialog, DialogContent, DialogFooter, Input, Select, toast } from "@/design";
import * as api from "@/api";
import { childKinds, kindLabel, validName } from "@/kinds";

const classes = [
  { value: "normal", label: "Normal" },
  { value: "personal", label: "Personal" },
  { value: "sealed", label: "Sealed" },
];
const classNote: Record<string, string> = {
  normal: "Ordinary data.",
  personal: "About people. Handled with care.",
  sealed: "Opaque bytes: never scanned, read or indexed.",
};

/** Make something inside `parent` (the top of your space when there is none). */
export function NewDialog({ open, onOpenChange, parent, where, onCreated }: { open: boolean; onOpenChange: (o: boolean) => void; parent: api.Securable | null; where: string; onCreated: (s: api.Securable) => void }) {
  const kinds = childKinds(parent?.kind ?? null);
  const [kind, setKind] = useState<api.Kind>(kinds[0] ?? "catalog");
  const [name, setName] = useState("");
  const [classification, setClassification] = useState("normal");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (open) {
      setKind(kinds[0] ?? "catalog");
      setName("");
      setClassification("normal");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, parent?.id]);

  const bad = name !== "" && !validName(name);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent title={`New ${kinds.length === 1 ? kindLabel[kinds[0]!].toLowerCase() : "item"}`} description={where}>
        <form
          className="mt-4 space-y-4"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!validName(name)) return;
            setBusy(true);
            try {
              const made = await api.create({ kind, parent_id: parent?.id ?? "", name, classification });
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
          <label className="block space-y-1.5">
            <span className="text-xs font-medium text-foreground">Classification</span>
            <Select aria-label="Classification" value={classification} onValueChange={setClassification} options={classes} />
            <span className="block text-2xs text-muted">{classNote[classification]}</span>
          </label>
          <DialogFooter>
            <Button type="submit" variant="primary" disabled={busy || !validName(name)}>
              Create
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
