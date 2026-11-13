import { useEffect, useState } from "react";
import { Button, Textarea, toast } from "@/design";
import * as api from "@/api";
import { useLoad } from "@/hooks";

/** Which rows of a table a person may see, and which columns are shown masked. Only whoever manages the table sees or changes this. */
export function TableRules({ ws, id }: { ws: string; id: string }) {
  const saved = useLoad(() => api.tableRules(ws, id), `${ws}/${id}/rules`);
  const [filter, setFilter] = useState("");
  const [masks, setMasks] = useState<{ column: string; rule: string }[]>([]);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    setFilter(saved.data?.row_filter ?? "");
    setMasks(Object.entries(saved.data?.masks ?? {}).map(([column, rule]) => ({ column, rule })));
  }, [saved.data]);

  if (saved.error) {
    const mine = saved.error.status === 403 || saved.error.status === 404;
    return <p className="max-w-lg text-sm text-muted">{mine ? "Only whoever manages this table can see or change its rules." : saved.error.message}</p>;
  }
  const next = { row_filter: filter.trim(), masks: Object.fromEntries(masks.filter((m) => m.column.trim()).map((m) => [m.column.trim(), m.rule.trim()])) };
  const before = { row_filter: saved.data?.row_filter ?? "", masks: saved.data?.masks ?? {} };
  const changed = JSON.stringify(next) !== JSON.stringify(before);
  const set = (i: number, part: Partial<{ column: string; rule: string }>) => setMasks(masks.map((m, j) => (j === i ? { ...m, ...part } : m)));

  return (
    <div className="flex max-w-2xl flex-col gap-5">
      <p className="text-sm text-muted">
        Rules apply to everyone who reads this table with SQL, except whoever manages it. A table with rules cannot be read in a job or by an Iceberg client.
      </p>
      <section className="flex flex-col gap-2">
        <h3 className="text-sm font-medium">Row filter</h3>
        <p className="text-xs text-muted">Only rows where this is true are visible. Leave empty to show every row.</p>
        <Textarea rows={3} spellCheck={false} className="font-mono text-sm" placeholder="region = 'EU' AND (owner = current_principal() OR in_group('finance'))" value={filter} onChange={(e) => setFilter(e.target.value)} />
      </section>
      <section className="flex flex-col gap-2">
        <h3 className="text-sm font-medium">Masked columns</h3>
        <p className="text-xs text-muted">Show a different value in place of a column, for example md5(email) or concat('****', right(card, 4)).</p>
        {masks.map((m, i) => (
          <div key={i} className="flex items-center gap-2">
            <input aria-label="Column" className="h-9 w-40 rounded-lg border border-border bg-background px-3 font-mono text-sm" placeholder="column" value={m.column} onChange={(e) => set(i, { column: e.target.value })} />
            <input aria-label="Shown as" className="h-9 flex-1 rounded-lg border border-border bg-background px-3 font-mono text-sm" placeholder="md5(column)" value={m.rule} onChange={(e) => set(i, { rule: e.target.value })} />
            <Button variant="ghost" onClick={() => setMasks(masks.filter((_, j) => j !== i))}>
              Remove
            </Button>
          </div>
        ))}
        <div>
          <Button variant="ghost" onClick={() => setMasks([...masks, { column: "", rule: "" }])}>
            Add a masked column
          </Button>
        </div>
      </section>
      <p className="text-xs text-muted">
        You can use the table's columns, values, comparisons, simple functions (lower, upper, md5, concat, left, right, coalesce, regexp_replace and a few more), <code>current_principal()</code> for the person reading, and <code>in_group('name')</code>.
      </p>
      <div>
        <Button
          variant="primary"
          disabled={!changed || busy}
          onClick={async () => {
            setBusy(true);
            try {
              await api.setTableRules(ws, id, next);
              saved.reload();
              toast.success("Rules saved");
            } catch (err) {
              toast.error("Could not save the rules", (err as Error).message);
            } finally {
              setBusy(false);
            }
          }}
        >
          Save rules
        </Button>
      </div>
    </div>
  );
}
