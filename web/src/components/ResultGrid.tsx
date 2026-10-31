import { Download } from "lucide-react";
import { Button } from "@/design";
import type { Run } from "@/api";
import { cellText, toCsv } from "@/format";

type Result = NonNullable<Run["result"]>;

const MAX_SHOWN = 2000;
const numeric = (type: string) => /INT|DECIMAL|DOUBLE|FLOAT|REAL|NUMERIC|HUGEINT/i.test(type);

const show = cellText;

export function ResultGrid({ result, elapsed }: { result: Result; elapsed?: string }) {
  const shown = result.rows.slice(0, MAX_SHOWN);
  const download = () => {
    const url = URL.createObjectURL(new Blob([toCsv(result.columns, result.rows)], { type: "text/csv" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = "result.csv";
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 10_000);
  };
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 items-center gap-3 border-b border-border px-4 py-1.5 text-xs text-muted">
        <span className="tabular-nums">
          {result.rows.length.toLocaleString()} {result.rows.length === 1 ? "row" : "rows"}
        </span>
        {elapsed && <span>{elapsed}</span>}
        {result.truncated && <span className="text-warning">Cut at the row limit: there are more</span>}
        {result.rows.length > MAX_SHOWN && <span>Showing the first {MAX_SHOWN.toLocaleString()}</span>}
        <span className="flex-1" />
        <Button variant="ghost" size="sm" onClick={download}>
          <Download /> CSV
        </Button>
      </div>
      <div className="scroll-area min-h-0 flex-1">
        <table className="w-max min-w-full border-separate border-spacing-0 text-xs">
          <thead className="sticky top-0 z-10 bg-surface">
            <tr>
              <th className="w-10 border-b border-border px-3 py-1.5 text-right font-normal text-muted-foreground">#</th>
              {result.columns.map((c, i) => (
                <th key={i} className={`border-b border-border px-3 py-1.5 font-medium text-foreground ${numeric(c.type) ? "text-right" : "text-left"}`}>
                  <span>{c.name}</span> <span className="font-normal text-muted-foreground">{c.type.toLowerCase()}</span>
                </th>
              ))}
            </tr>
          </thead>
          <tbody className="font-mono">
            {shown.map((row, r) => (
              <tr key={r} className="hover:bg-card-hover">
                <td className="border-b border-border/60 px-3 py-1 text-right text-muted-foreground">{r + 1}</td>
                {result.columns.map((c, i) => {
                  const v = row[i];
                  return (
                    <td key={i} title={show(v)} className={`max-w-96 truncate border-b border-border/60 px-3 py-1 ${numeric(c.type) ? "text-right tabular-nums" : "text-left"} ${v === null ? "italic text-muted-foreground" : "text-foreground"}`}>
                      {v === null ? "null" : show(v)}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
        {result.rows.length === 0 && <p className="px-4 py-6 text-center text-xs text-muted">The query ran and returned no rows.</p>}
      </div>
    </div>
  );
}
