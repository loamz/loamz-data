import type { ReactNode } from "react";
import { cn } from "../cn";
import { Input } from "../ui/Input";

export interface Column<T> {
  key: string;
  header: string;
  cell: (row: T) => ReactNode;
  /** `fit` takes what its content needs, `fill` takes what is left (one column per table), a number is a fixed width in rem. */
  width?: "fit" | "fill" | number;
  align?: "left" | "right";
}

/**
 * One table, as dense as the data deserves: 36px rows, a header that stays, columns that take only the width they need so a few columns
 * do not drift across the whole screen. `onOpen` makes a row a link to its object; `selected` marks the row whose details show beside it.
 */
export function DataTable<T>({
  columns,
  rows,
  rowKey,
  onOpen,
  selected,
  filter,
  onFilter,
  count,
  empty,
  className,
}: {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T) => string;
  onOpen?: (row: T) => void;
  selected?: string | null;
  filter?: string;
  onFilter?: (v: string) => void;
  /** Shown beside the filter: "7 schemas". */
  count?: string;
  empty?: ReactNode;
  className?: string;
}) {
  const w = (c: Column<T>): string | undefined => (typeof c.width === "number" ? `${c.width}rem` : c.width === "fill" ? undefined : "1%");
  return (
    <div className={cn("overflow-hidden rounded-lg border border-border bg-background/40", className)}>
      {onFilter && (
        <div className="flex items-center gap-3 border-b border-border p-2">
          <Input size="md" className="max-w-64" placeholder="Filter" aria-label="Filter" value={filter ?? ""} onChange={(e) => onFilter(e.target.value)} />
          {count && <span className="text-sm text-muted">{count}</span>}
        </div>
      )}
      <div className="scroll-area max-h-full overflow-x-auto">
        <table className="w-full border-collapse text-sm">
          <thead className="sticky top-0 bg-card text-xs text-muted">
            <tr className="h-control-md">
              {columns.map((c) => (
                <th key={c.key} scope="col" style={{ width: w(c) }} className={cn("whitespace-nowrap px-3 text-left font-medium", c.align === "right" && "text-right")}>
                  {c.header}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => {
              const k = rowKey(r);
              return (
                <tr
                  key={k}
                  aria-selected={selected === k || undefined}
                  onClick={onOpen ? () => onOpen(r) : undefined}
                  className={cn("h-9 border-t border-border", onOpen && "cursor-pointer", selected === k ? "bg-accent/20" : "hover:bg-card-hover")}
                >
                  {columns.map((c) => (
                    <td key={c.key} className={cn("px-3 align-middle", c.width !== "fill" && "whitespace-nowrap", c.width === "fill" && "max-w-0 truncate", c.align === "right" && "text-right tabular-nums")}>
                      {c.cell(r)}
                    </td>
                  ))}
                </tr>
              );
            })}
            {rows.length === 0 && (
              <tr>
                <td colSpan={columns.length} className="px-3 py-6 text-center text-muted">
                  {empty ?? "Nothing here"}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
